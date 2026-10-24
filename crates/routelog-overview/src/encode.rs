//! フレームを ffmpeg に渡して、MP4 に書く。
//!
//! 止まってからのフレームは同じ絵なので、1 枚だけ渡して ffmpeg に繰り返させる。
//!
//! 書くのは、使えれば GPU（NVIDIA の NVENC）、使えなければ CPU（libx264）にする。

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::Error;

/// ffmpeg の場所を上書きする環境変数。
const PROGRAM_ENV: &str = "ROUTELOG_FFMPEG";
/// 書き方を決めてしまう環境変数。`nvenc` か `x264`。無ければ、NVENC が使えるかを試して決める。
const CODEC_ENV: &str = "ROUTELOG_VIDEO_CODEC";

/// 色の決まり。絵に付けておかないと、書き出しの設定で指定しても、原色と伝達特性が「不明」になる。
const COLOR: &str = "setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv";

/// 動画の書き方。どちらも H.264（High）にする。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// NVIDIA の GPU で書く
    Nvenc,
    /// CPU で書く
    X264,
}

impl Codec {
    fn args(self) -> &'static [&'static str] {
        match self {
            // 画質を一定に保つ設定。CPU の crf 16 と見比べて、同じくらいの見た目になる値にした。
            Codec::Nvenc => &[
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p7",
                "-tune",
                "hq",
                "-rc",
                "vbr",
                "-cq",
                "19",
                "-b:v",
                "0",
                "-spatial-aq",
                "1",
                "-bf",
                "2",
            ],
            Codec::X264 => &["-c:v", "libx264", "-preset", "medium", "-crf", "16"],
        }
    }
}

#[derive(Debug)]
pub struct Ffmpeg {
    program: OsString,
    codec: OnceLock<Codec>,
}

impl Ffmpeg {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            codec: OnceLock::new(),
        }
    }

    /// 環境変数 `ROUTELOG_FFMPEG`、フォルダ `dir` の中、PATH の順に探す。
    pub fn find(dir: Option<&Path>) -> Self {
        Self::new(routelog_video::find_program(PROGRAM_ENV, "ffmpeg", dir))
    }

    /// 使う書き方。最初に呼んだときに決め、後は覚えておく。
    pub fn codec(&self) -> Codec {
        *self
            .codec
            .get_or_init(|| match std::env::var(CODEC_ENV).as_deref() {
                Ok("nvenc") => Codec::Nvenc,
                Ok("x264") => Codec::X264,
                _ if self.nvenc_works() => Codec::Nvenc,
                _ => Codec::X264,
            })
    }

    /// NVENC で書けるか。ffmpeg に入っていても、GPU や driver が無ければ書けないので、小さな絵を
    /// 書かせて確かめる。
    fn nvenc_works(&self) -> bool {
        let mut command = Command::new(&self.program);
        command
            .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi"])
            .args(["-i", "color=c=gray:s=256x256:r=60:d=0.1"])
            .args(Codec::Nvenc.args())
            .args(["-f", "null", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        routelog_video::hide_console(&mut command);
        command.status().is_ok_and(|status| status.success())
    }

    /// `out` に書き始める。渡したフレームの後に、最後のフレームを `repeat` 枚ぶん足す。
    pub fn start(
        &self,
        out: &Path,
        size: [u32; 2],
        fps: u32,
        repeat: u32,
    ) -> Result<Encoder, Error> {
        let mut command = Command::new(&self.program);
        command
            .args(["-hide_banner", "-loglevel", "error", "-nostats", "-progress", "pipe:1", "-y"])
            .args(["-f", "rawvideo", "-pix_fmt", "rgba"])
            .args(["-s", &format!("{}x{}", size[0], size[1])])
            .args(["-framerate", &fps.to_string(), "-i", "-"])
            // 色を変えてから繰り返す。繰り返した絵を、1 枚ずつ変え直さない。
            .args([
                "-vf",
                &format!(
                    "scale=out_color_matrix=bt709:out_range=tv,format=yuv420p,{COLOR},tpad=stop_mode=clone:stop={repeat}"
                ),
            ])
            .args(self.codec().args())
            .args(["-profile:v", "high", "-level:v", "5.2"])
            .args(["-movflags", "+faststart", "-an"])
            .arg(out)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        routelog_video::hide_console(&mut command);
        let mut child = command.spawn().map_err(|source| Error::Spawn {
            program: self.program.to_string_lossy().into_owned(),
            source,
        })?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("標準出力をつないである");
        let mut stderr = child.stderr.take().expect("標準エラーをつないである");
        let errors = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });
        // 進み具合は書いている間ずっと出てくる。読まずにおくと、管がいっぱいになって ffmpeg が止まる。
        let encoded = Arc::new(AtomicU32::new(0));
        {
            let encoded = Arc::clone(&encoded);
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Some(frame) = line
                        .strip_prefix("frame=")
                        .and_then(|n| n.trim().parse().ok())
                    {
                        encoded.store(frame, Ordering::Relaxed);
                    }
                }
            });
        }
        Ok(Encoder {
            child,
            stdin,
            encoded,
            errors: Some(errors),
        })
    }
}

/// 書いている途中の動画。
pub struct Encoder {
    child: Child,
    stdin: Option<ChildStdin>,
    /// 書き上がったフレームの数
    encoded: Arc<AtomicU32>,
    errors: Option<JoinHandle<String>>,
}

impl Encoder {
    /// フレームを 1 枚渡す。RGBA を、上の行から並べたもの。
    pub fn write(&mut self, rgba: &[u8]) -> Result<(), Error> {
        let stdin = self.stdin.as_mut().expect("渡し終える前にだけ呼ぶ");
        if stdin.write_all(rgba).is_err() {
            // ffmpeg が先に止まった。わけは、終わらせたときの標準エラーにある。
            return Err(self.fail());
        }
        Ok(())
    }

    /// フレームを渡し終えて、書き終わるのを待つ。書き上がったフレームの数を、進むたびに知らせる。
    pub fn finish(
        mut self,
        on_frame: &mut dyn FnMut(u32),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        drop(self.stdin.take());
        loop {
            on_frame(self.encoded());
            if cancelled() {
                self.abort();
                return Err(Error::Cancelled);
            }
            match self
                .child
                .try_wait()
                .map_err(|e| Error::Encode(e.to_string()))?
            {
                Some(status) if status.success() => return Ok(()),
                Some(_) => return Err(self.fail()),
                None => std::thread::sleep(Duration::from_millis(200)),
            }
        }
    }

    /// 書き上がったフレームの数。
    pub fn encoded(&self) -> u32 {
        self.encoded.load(Ordering::Relaxed)
    }

    /// 止める。書きかけのファイルは、呼んだ側が消す。
    pub fn abort(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn fail(&mut self) -> Error {
        drop(self.stdin.take());
        let _ = self.child.wait();
        let text = self
            .errors
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        Error::Encode(text.trim().to_owned())
    }
}
