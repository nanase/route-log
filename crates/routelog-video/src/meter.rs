//! 停車を見分けるための値を、ffmpeg で動画から測る。
//!
//! 2 段で測る。まず動画の全体を読み、1 チャンネル目の音の大きさと、キーフレーム（0.5 秒おき）の
//! 差を測る。キーフレームだけなら、1 時間の動画を GPU で 1 分あまりで読める。次に、音が大きく
//! 止まっていそうな所の周りだけ、全部のフレームを読んで 0.1 秒ごとの差を測る。
//!
//! 動きは、1/12 に縮めたグレースケールの絵で、明るさが 12 より大きく変わった画素の割合で測る。
//! GPU が使えなければ、ffmpeg が CPU で読む（`-hwaccel auto`）。

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::ops::Range;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::stops::{MEASURE_VERSION, Measured, windows};
use crate::{Error, find_ffmpeg, hide_console};
use Sample::{Motion, Sound};

/// 絵を縮めて、隣の絵との差を取り、明るさが 12 より大きく変わった画素を白にする。
/// 白の割合は、明るさの平均（YAVG）を 2.55 で割ると % になる。
const MOTION: &str = "scale=160:90,format=gray,tblend=all_mode=difference,lut=y='if(gt(val,12),255,0)',signalstats,metadata@motion=print:key=lavfi.signalstats.YAVG";
/// 1 チャンネル目だけを、1 秒ごとに測る。両方のチャンネルに同じ音が入ったファイルもあり、
/// 混ぜると片方だけのファイルと大きさが変わる。
const SOUND: &str = "pan=mono|c0=c0,asetnsamples=48000,astats=metadata=1:reset=1:measure_perchannel=none:measure_overall=RMS_level,ametadata@sound=print:key=lavfi.astats.Overall.RMS_level";

/// 全体を読む段が、進み具合のうちに占める割合。細かく測る段は残りを使う。
const FIRST_PASS_SHARE: f64 = 0.6;

#[derive(Debug, Clone)]
pub struct StopMeter {
    program: OsString,
}

/// ffmpeg が書いた、測った値 1 つ。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Sample {
    /// 音の大きさ（dB）
    Sound(f64, f32),
    /// 動いた画素の割合（%）
    Motion(f64, f32),
}

/// 測った値を読んでいる途中の状態。フィルタは、時刻の行と値の行を分けて書く。
#[derive(Default)]
struct Reader {
    sound_at: Option<f64>,
    motion_at: Option<f64>,
}

impl Reader {
    /// ffmpeg の標準エラーの 1 行を読む。値の行なら、その値と時刻を返す。
    fn line(&mut self, line: &str) -> Option<Sample> {
        let (slot, body) = if let Some(rest) = line.strip_prefix("[ametadata@sound ") {
            (&mut self.sound_at, rest)
        } else {
            (&mut self.motion_at, line.strip_prefix("[metadata@motion ")?)
        };
        let body = body.split_once("] ")?.1.trim();
        if let Some(time) = body.split_once("pts_time:") {
            *slot = time.1.trim().parse().ok();
            return None;
        }
        let (key, value) = body.split_once('=')?;
        let at = slot.take()?;
        // 無音は -inf になる。
        let value: f32 = match value.trim() {
            "-inf" => -120.0,
            v => v.parse().ok()?,
        };
        Some(if key.ends_with("RMS_level") {
            Sound(at, value.max(-120.0))
        } else {
            Motion(at, value / 2.55)
        })
    }
}

impl StopMeter {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
        }
    }

    /// 環境変数 `ROUTELOG_FFMPEG`、フォルダ `dir` の中、PATH の順に探す。
    pub fn find(dir: Option<&Path>) -> Self {
        Self::new(find_ffmpeg(dir))
    }

    /// 動画 1 本を測る。進み具合（0〜1）を `progress` に知らせる。`cancelled` が真になったらやめる。
    ///
    /// 動画のファイルは読むだけで、何も書かない。
    pub fn measure(
        &self,
        path: &Path,
        duration_ms: i64,
        progress: &mut dyn FnMut(f64),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Measured, Error> {
        let seconds = (duration_ms.max(0) as usize).div_ceil(1000);
        let duration = (duration_ms as f64 / 1000.0).max(1.0);

        let mut audio: Vec<Vec<f32>> = vec![Vec::new(); seconds];
        let mut coarse: Vec<Vec<f32>> = vec![Vec::new(); seconds];
        let first = self.command(path, None, &format!("[0:v:0]{MOTION}[v];[0:a:0]{SOUND}[a]"));
        self.run(first, cancelled, &mut |sample| {
            let (at, value, into) = match sample {
                Sound(at, db) => (at, db, &mut audio),
                Motion(at, pct) => (at, pct, &mut coarse),
            };
            if let Some(second) = into.get_mut(at as usize) {
                second.push(value);
            }
            progress(FIRST_PASS_SHARE * (at / duration).min(1.0));
        })?;
        // 値の無い秒は、音は無音、動きは大きく動いたものとする。どちらも候補にならない。
        let audio_db: Vec<f32> = audio
            .iter()
            .map(|v| v.first().copied().map_or(-120.0, round))
            .collect();
        let coarse_pct: Vec<f32> = coarse
            .iter()
            .map(|v| v.iter().copied().reduce(f32::min).map_or(100.0, round))
            .collect();

        let windows = windows(&audio_db, &coarse_pct);
        let total: usize = windows.iter().map(Range::len).sum();
        let mut fine: Vec<Vec<f32>> = vec![Vec::new(); seconds];
        let mut done = 0;
        for window in windows {
            let second = self.command(
                path,
                Some(window.clone()),
                &format!("[0:v:0]fps=10,{MOTION}[v]"),
            );
            self.run(second, cancelled, &mut |sample| {
                if let Motion(at, pct) = sample {
                    if let Some(second) = fine.get_mut(window.start + at as usize) {
                        second.push(pct);
                    }
                    let measured =
                        (done as f64 + at.min(window.len() as f64)) / total.max(1) as f64;
                    progress(FIRST_PASS_SHARE + (1.0 - FIRST_PASS_SHARE) * measured);
                }
            })?;
            done += window.len();
        }
        let fine_pct = fine
            .iter()
            .map(|v| (!v.is_empty()).then(|| round(v.iter().sum::<f32>() / v.len() as f32)))
            .collect();
        progress(1.0);
        Ok(Measured {
            version: MEASURE_VERSION,
            audio_db,
            coarse_pct,
            fine_pct,
        })
    }

    /// `window` の秒だけを読む ffmpeg。無ければ全体を、キーフレームだけ読む。
    fn command(&self, path: &Path, window: Option<Range<usize>>, graph: &str) -> Command {
        let mut command = Command::new(&self.program);
        // 測った値は、フィルタの名前を付けてログに出る。ログの水準を info より下げない。
        command.args(["-hide_banner", "-nostdin", "-nostats", "-loglevel", "info"]);
        command.args(["-hwaccel", "auto"]);
        match &window {
            Some(window) => {
                command.args([
                    "-ss",
                    &window.start.to_string(),
                    "-t",
                    &window.len().to_string(),
                ]);
            }
            None => {
                command.args(["-skip_frame", "nokey"]);
            }
        }
        command.arg("-i").arg(path).args(["-filter_complex", graph]);
        command.args(["-map", "[v]", "-f", "null", "-"]);
        if window.is_none() {
            command.args(["-map", "[a]", "-f", "null", "-"]);
        }
        command
    }

    /// ffmpeg を動かし、測った値を 1 つずつ `on_sample` に渡す。
    fn run(
        &self,
        mut command: Command,
        cancelled: &dyn Fn() -> bool,
        on_sample: &mut dyn FnMut(Sample),
    ) -> Result<(), Error> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        hide_console(&mut command);
        let mut child = command.spawn().map_err(|source| Error::Spawn {
            program: self.program.to_string_lossy().into_owned(),
            source,
        })?;
        let mut stderr = BufReader::new(child.stderr.take().expect("標準エラーをつないである"));
        let mut reader = Reader::default();
        // 失敗したときに見せる、値でない行。最後のほうだけを残す。
        let mut messages: Vec<String> = Vec::new();
        let mut line = Vec::new();
        loop {
            if cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Cancelled);
            }
            line.clear();
            // ファイル名に UTF-8 でない字があっても、読むのをやめない。
            match stderr.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let text = String::from_utf8_lossy(&line);
            match reader.line(&text) {
                Some(sample) => on_sample(sample),
                None if !text.starts_with('[') || text.contains("rror") => {
                    messages.push(text.trim_end().to_owned());
                    if messages.len() > 20 {
                        messages.remove(0);
                    }
                }
                None => {}
            }
        }
        let status = child.wait().map_err(|e| Error::Ffmpeg(e.to_string()))?;
        if !status.success() {
            return Err(Error::Ffmpeg(messages.join("\n")));
        }
        Ok(())
    }
}

/// 持つ値は 0.01 の桁までにする。
fn round(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_values_that_the_filters_print() {
        let mut reader = Reader::default();
        let lines = [
            "[ametadata@sound @ 000001D4744E5500] frame:0    pts:0       pts_time:0",
            "[ametadata@sound @ 000001D4744E5500] lavfi.astats.Overall.RMS_level=-14.074458",
            "[metadata@motion @ 000001D4744E4000] frame:0    pts:29029   pts_time:0.483817",
            "[metadata@motion @ 000001D4744E4000] lavfi.signalstats.YAVG=127.5",
            "[ametadata@sound @ 000001D4744E5500] frame:1    pts:48000   pts_time:1",
            "[Parsed_astats_8 @ 000001D4744E4300] RMS level dB: -14.446650",
            "[ametadata@sound @ 000001D4744E5500] lavfi.astats.Overall.RMS_level=-inf",
            "Stream mapping:",
        ];
        let samples: Vec<Sample> = lines.iter().filter_map(|l| reader.line(l)).collect();
        assert_eq!(
            samples,
            [
                Sound(0.0, -14.074458),
                Motion(0.483817, 50.0),
                Sound(1.0, -120.0)
            ]
        );
    }

    #[test]
    fn a_value_without_its_time_is_skipped() {
        let mut reader = Reader::default();
        assert_eq!(
            reader.line("[metadata@motion @ 0] lavfi.signalstats.YAVG=12"),
            None
        );
    }

    #[test]
    fn a_missing_program_says_which_one() {
        let error = StopMeter::new("routelog-no-such-ffmpeg")
            .measure(Path::new("x.MP4"), 1000, &mut |_| {}, &|| false)
            .unwrap_err();
        assert!(error.to_string().contains("routelog-no-such-ffmpeg"));
    }

    #[test]
    fn the_first_pass_reads_only_keyframes_and_both_streams() {
        let meter = StopMeter::new("ffmpeg");
        let args = |command: Command| -> Vec<String> {
            command
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        let first = args(meter.command(Path::new("a.MP4"), None, "g"));
        assert!(first.windows(2).any(|w| w == ["-skip_frame", "nokey"]));
        assert_eq!(first.iter().filter(|a| *a == "-map").count(), 2);
        assert!(!first.contains(&"-ss".to_owned()));

        let second = args(meter.command(Path::new("a.MP4"), Some(30..50), "g"));
        assert!(second.windows(4).any(|w| w == ["-ss", "30", "-t", "20"]));
        assert!(!second.contains(&"-skip_frame".to_owned()));
        assert_eq!(second.iter().filter(|a| *a == "-map").count(), 1);
    }
}
