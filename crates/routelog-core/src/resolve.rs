//! 回を、Resolve のソースのタイムライン（FCPXML 1.10）と、字幕を出す所のマーカー（EDL）にする。
//!
//! 無料版の Resolve はスクリプトで操作できないので、画面から読み込めるファイルを書く。停車は
//! 抜き、元の動画のタイムコードで素材を指す。時間はすべて 59.94 fps のフレームの数で持つ。
//!
//! タイムラインは、Resolve の既定と同じ 01:00:00;00 から始める。パートのタイムラインも既定の
//! まま作り、ソースを頭から丸ごと包むので、同じマーカーをパートに読み込んでも同じ場面に来る。

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::episode::Extent;
use crate::estimate::{self, Stop};
use crate::event::EventKind;
use crate::ride::RideDocument;
use crate::timeline::{Timeline, VideoFile};

/// 素材とタイムラインのフレームレート（60000/1001）。
const FRAME_RATE: (u32, u32) = (60000, 1001);

/// 1 フレームの長さ。
const FRAME: &str = "1001/60000s";

/// タイムラインの始まり（01:00:00;00）のフレームの数。
const START_FRAMES: i64 = 215_784;

/// 書き出すタイムラインの大きさ。YouTube で十分なビットレートを得るため、素材より大きい 4K にする。
const SEQUENCE_SIZE: (u32, u32) = (3840, 2160);

/// 動画ファイル 1 本について ffprobe が答えたこと。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    /// 映像のフレームレートの分子と分母。
    pub frame_rate: (u32, u32),
    /// 映像のフレームの数。
    pub frames: i64,
    pub width: u32,
    pub height: u32,
    /// 音のチャンネルの数。音が無ければ 0。
    pub audio_channels: u32,
    /// 埋め込まれた開始のタイムコード。`05:05:26;07` の形。
    pub timecode: Option<String>,
}

/// 書き出せない理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// 始まりか終わりの経由点を通った時刻が、推定できていない。
    NoTimes,
    /// 回の範囲に、録画が無い。停車をすべて抜いた場合も含む。
    NotRecorded,
    /// 59.94 fps でない動画。ファイル名とフレームレート。
    FrameRate(Vec<(String, (u32, u32))>),
    /// 開始のタイムコードを読めない動画のファイル名。
    NoTimecode(Vec<String>),
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTimes => f.write_str(
                "始まりか終わりの経由地を通った時刻が推定できていません。前後のイベントの時刻を確定してください",
            ),
            Self::NotRecorded => f.write_str("パートの範囲に動画がありません"),
            Self::FrameRate(files) => {
                f.write_str("59.94 fps でない動画があります。Resolve のタイムラインは 59.94 fps です：")?;
                let listed: Vec<String> = files
                    .iter()
                    .map(|(name, (num, den))| format!("{name}（{} fps）", fps(*num, *den)))
                    .collect();
                f.write_str(&listed.join("、"))
            }
            Self::NoTimecode(names) => {
                write!(f, "タイムコードを読めない動画があります：{}", names.join("、"))
            }
        }
    }
}

impl std::error::Error for Problem {}

/// `30000/1001` を `29.97` のように見せる。
fn fps(num: u32, den: u32) -> String {
    if den == 0 {
        return "不明".into();
    }
    let text = format!("{:.3}", f64::from(num) / f64::from(den));
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// ミリ秒を、いちばん近いフレームの数にする。
fn ms_to_frames(ms: i64) -> i64 {
    (ms * 60 + 500).div_euclid(1001)
}

/// フレームの数をミリ秒にする。端数は切り捨てる。
pub fn frames_to_ms(frames: i64) -> i64 {
    (frames * 1001).div_euclid(60)
}

/// FCPXML の時間。
fn time(frames: i64) -> String {
    if frames == 0 {
        "0s".into()
    } else {
        format!("{}/60000s", frames * 1001)
    }
}

/// ドロップフレームのタイムコードをフレームの数にする。`nominal` は 30 か 60。
fn tc_to_frames(tc: &str, nominal: i64) -> Option<i64> {
    let parts: Vec<i64> = tc
        .split([':', ';', '.', ','])
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let [h, m, s, f] = parts[..] else { return None };
    if m >= 60 || s >= 60 || f >= nominal {
        return None;
    }
    let drop = nominal / 15;
    let minutes = h * 60 + m;
    Some((h * 3600 + m * 60 + s) * nominal + f - drop * (minutes - minutes / 10))
}

/// フレームの数を、ドロップフレームのタイムコードにする。`nominal` は 30 か 60。
fn frames_to_tc(frames: i64, nominal: i64) -> String {
    let drop = nominal / 15;
    let per_minute = nominal * 60 - drop;
    let per_ten = nominal * 600 - 9 * drop;
    let (tens, rest) = (frames / per_ten, frames % per_ten);
    let mut f = frames + 9 * drop * tens;
    if rest > drop {
        f += drop * ((rest - drop) / per_minute);
    }
    let (h, m, s, ff) = (
        f / (nominal * 3600),
        f / (nominal * 60) % 60,
        f / nominal % 60,
        f % nominal,
    );
    format!("{h:02}:{m:02}:{s:02};{ff:02}")
}

/// 素材のタイムコードの始まり（59.94 のフレームの数）。
///
/// カメラは、59.94 fps の映像にも 29.97 fps のドロップフレームでタイムコードを付ける。
fn start_frames(timecode: &str) -> Option<i64> {
    tc_to_frames(timecode, 30).map(|frames| frames * 2)
}

/// タイムラインに並べる素材 1 本。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Asset {
    /// タイムラインのファイルの番号。
    file: usize,
    name: String,
    path: String,
    /// 素材の頭のタイムコード（59.94 のフレームの数）。
    start_frames: i64,
    frames: i64,
    size: (u32, u32),
    audio_channels: u32,
}

/// 素材の中の、使う間 `[in_frames, out_frames)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Piece {
    /// `SourceTimeline::assets` の番号。
    asset: usize,
    in_frames: i64,
    out_frames: i64,
}

impl Piece {
    fn frames(&self) -> i64 {
        self.out_frames - self.in_frames
    }
}

/// タイムラインのマーカー。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Marker {
    /// タイムラインの頭からのフレームの数。
    frames: i64,
    kind: EventKind,
    name: String,
}

/// 停車を抜いた、回のソースのタイムライン。
#[derive(Debug, Clone, PartialEq)]
pub struct SourceTimeline {
    assets: Vec<Asset>,
    pieces: Vec<Piece>,
    markers: Vec<Marker>,
    /// 字幕にするイベントのうち、回の範囲にあって、録画の外か抜いた所にあるため置けなかったもの。
    pub unplaced: usize,
    /// 録画の中で抜いた停車の数と長さ（ミリ秒）。
    pub cuts: usize,
    pub cut_ms: i64,
}

/// 回の範囲で、録画しているファイルの番号。ffprobe で読むのはこれだけでよい。
pub fn files_in(timeline: &Timeline, extent: &Extent) -> Vec<usize> {
    let (Some(start), Some(end)) = (extent.start_ms, extent.end_ms) else {
        return Vec::new();
    };
    timeline
        .spans()
        .iter()
        .filter(|s| s.start_ms < end && start < s.end_ms)
        .map(|s| s.file)
        .collect()
}

/// 動画の中の位置で持つ区間（採った停車の候補）を、実時刻の区間にする。`within` は、その動画の
/// 区間 `[start_ms, end_ms)` を返す。
///
/// 後の動画が始まった所で前の動画を切っているので、区間もそこで切る。画面の `stopBands` と同じ。
pub fn real_cuts(
    timeline: &Timeline,
    mut within: impl FnMut(&VideoFile) -> Vec<(i64, i64)>,
) -> Vec<Stop> {
    let mut cuts = Vec::new();
    for span in timeline.spans() {
        for (start, end) in within(&timeline.files()[span.file]) {
            let start_ms = (span.start_ms + start).max(span.start_ms);
            let end_ms = (span.start_ms + end).min(span.end_ms);
            if start_ms < end_ms {
                cuts.push(Stop { start_ms, end_ms });
            }
        }
    }
    cuts
}

/// 実時刻の間 `[start, end)` から、抜く間を除いた残り。
fn subtract(start: i64, end: i64, cuts: &[Stop]) -> Vec<(i64, i64)> {
    let mut kept = vec![(start, end)];
    for cut in cuts {
        kept = kept
            .into_iter()
            .flat_map(|(s, e)| {
                [(s, e.min(cut.start_ms)), (s.max(cut.end_ms), e)]
                    .into_iter()
                    .filter(|(a, b)| a < b)
            })
            .collect();
    }
    kept
}

/// 種類の呼び名。名前の無いイベントのマーカーに使う。画面の `KINDS` と同じ。
fn kind_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Departure => "出発",
        EventKind::Arrival => "到着",
        EventKind::PrefBoundary => "県境",
        EventKind::MuniBoundary => "市区町村境",
        EventKind::Road => "路線",
        EventKind::Bridge => "橋",
        EventKind::Tunnel => "トンネル",
        EventKind::MichiNoEki => "道の駅",
        EventKind::Waypoint => "経由地",
        EventKind::Stop => "停車",
        EventKind::Custom => "手動",
    }
}

/// マーカーを、通った時刻より前へずらす長さ（ミリ秒）。字幕はフェードインさせて入れるので、少し
/// 早めに置く。種類はマーカーの色と同じに分ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MarkerLeads {
    /// 出発・到着
    pub endpoint: i64,
    pub road: i64,
    /// 県境・市区町村境
    pub boundary: i64,
    pub bridge: i64,
    pub tunnel: i64,
    pub michi_no_eki: i64,
    pub other: i64,
}

impl Default for MarkerLeads {
    /// どれも 2 秒。利用者は、字幕を 1 秒ほどかけて出すため、2 秒ほど前に置いている。
    fn default() -> Self {
        Self {
            endpoint: 2000,
            road: 2000,
            boundary: 2000,
            bridge: 2000,
            tunnel: 2000,
            michi_no_eki: 2000,
            other: 2000,
        }
    }
}

impl MarkerLeads {
    fn of(&self, kind: EventKind) -> i64 {
        match kind {
            EventKind::Departure | EventKind::Arrival => self.endpoint,
            EventKind::Road => self.road,
            EventKind::PrefBoundary | EventKind::MuniBoundary => self.boundary,
            EventKind::Bridge => self.bridge,
            EventKind::Tunnel => self.tunnel,
            EventKind::MichiNoEki => self.michi_no_eki,
            _ => self.other,
        }
    }
}

/// Resolve のマーカーの色。種類で分ける。`MarkerLeads` も同じ分け方にする。
fn color(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Departure | EventKind::Arrival => "ResolveColorRed",
        EventKind::Road => "ResolveColorBlue",
        EventKind::PrefBoundary | EventKind::MuniBoundary => "ResolveColorGreen",
        EventKind::Bridge => "ResolveColorCyan",
        EventKind::Tunnel => "ResolveColorPurple",
        EventKind::MichiNoEki => "ResolveColorYellow",
        _ => "ResolveColorCream",
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// ファイルの場所を file URL にする。空白や日本語は、UTF-8 のバイトごとにパーセントで書く。
fn file_url(path: &str) -> String {
    let path = path.replace('\\', "/");
    let mut url = String::from(if path.starts_with('/') {
        "file://"
    } else {
        "file:///"
    });
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            url.push(byte as char);
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    url
}

/// 回の範囲の素材を、ffprobe が答えたことから作る。59.94 fps でない素材と、タイムコードを
/// 読めない素材があれば、書き出さない。
fn assets(
    timeline: &Timeline,
    used: &[usize],
    media: &[Option<Media>],
) -> Result<Vec<Asset>, Problem> {
    let files = timeline.files();
    let read: Vec<(usize, Option<&Media>)> = used
        .iter()
        .map(|&i| (i, media.get(i).and_then(Option::as_ref)))
        .collect();
    let slow: Vec<(String, (u32, u32))> = read
        .iter()
        .filter_map(|&(i, m)| Some((i, m?)))
        .filter(|(_, m)| m.frame_rate != FRAME_RATE)
        .map(|(i, m)| (files[i].name.clone(), m.frame_rate))
        .collect();
    if !slow.is_empty() {
        return Err(Problem::FrameRate(slow));
    }
    let mut assets = Vec::new();
    let mut unreadable = Vec::new();
    for (i, m) in read {
        let start = m.and_then(|m| Some((m, start_frames(m.timecode.as_deref()?)?)));
        match start {
            Some((m, start_frames)) => assets.push(Asset {
                file: i,
                name: files[i].name.clone(),
                path: files[i].path.clone(),
                start_frames,
                frames: m.frames,
                size: (m.width, m.height),
                audio_channels: m.audio_channels,
            }),
            None => unreadable.push(files[i].name.clone()),
        }
    }
    if unreadable.is_empty() {
        Ok(assets)
    } else {
        Err(Problem::NoTimecode(unreadable))
    }
}

/// 素材を `[start_ms, end_ms)` で切り、`cuts` を抜いて並べる。抜いた所の数と長さも返す。
fn cut_pieces(
    timeline: &Timeline,
    assets: &[Asset],
    (start_ms, end_ms): (i64, i64),
    cuts: &[Stop],
) -> (Vec<Piece>, usize, i64) {
    let mut pieces = Vec::new();
    let (mut count, mut cut_ms) = (0, 0);
    for span in timeline.spans() {
        let Some(asset) = assets.iter().position(|a| a.file == span.file) else {
            continue;
        };
        let (from, to) = (span.start_ms.max(start_ms), span.end_ms.min(end_ms));
        let kept = subtract(from, to, cuts);
        cut_ms += (to - from) - kept.iter().map(|(s, e)| e - s).sum::<i64>();
        // 範囲の中で抜いた所の数。端で切れた停車も 1 つと数える。
        count += cuts
            .iter()
            .filter(|c| c.start_ms < to && from < c.end_ms)
            .count();
        pieces.extend(kept.into_iter().map(|(s, e)| Piece {
            asset,
            in_frames: ms_to_frames(s - span.start_ms),
            out_frames: ms_to_frames(e - span.start_ms).min(assets[asset].frames),
        }));
    }
    pieces.retain(|p| p.in_frames < p.out_frames);
    (pieces, count, cut_ms)
}

/// 字幕にするイベントのうち、経路の上で回の範囲にあるものを、タイムラインのマーカーにする。
/// 録画の外か、抜いた所にあって置けなかったイベントの数も返す。
fn place_markers(
    doc: &RideDocument,
    extent: &Extent,
    timeline: &Timeline,
    assets: &[Asset],
    pieces: &[Piece],
) -> (Vec<Marker>, usize) {
    let at = |real_ms: i64| {
        let position = timeline.locate(real_ms)?;
        let frames = ms_to_frames(position.offset_ms);
        let mut offset = 0;
        for piece in pieces {
            if assets[piece.asset].file == position.file
                && piece.in_frames <= frames
                && frames < piece.out_frames
            {
                return Some(offset + frames - piece.in_frames);
            }
            offset += piece.frames();
        }
        None
    };
    let mut markers = Vec::new();
    let mut unplaced = 0;
    let shown = doc
        .events
        .iter()
        .filter(|e| !e.hidden && extent.start_m <= e.route_m && e.route_m <= extent.end_m);
    for event in shown {
        match event.time_ms.and_then(at) {
            Some(frames) => markers.push(Marker {
                frames,
                kind: event.kind,
                name: if event.label.trim().is_empty() {
                    kind_name(event.kind).to_owned()
                } else {
                    event.label.trim().to_owned()
                },
            }),
            None => unplaced += 1,
        }
    }
    markers.sort_by_key(|m| m.frames);
    (markers, unplaced)
}

impl SourceTimeline {
    /// 回の範囲から、停車を抜いたタイムラインを作る。
    ///
    /// 抜くのは、走行の停車のイベントと `cuts`（採った停車の候補。実時刻）で、録画している所
    /// だけが効く。`media` はファイルの番号で引き、`files_in` が返したファイルの分があればよい。
    pub fn new(
        doc: &RideDocument,
        extent: &Extent,
        timeline: &Timeline,
        media: &[Option<Media>],
        cuts: &[Stop],
    ) -> Result<Self, Problem> {
        let (Some(start_ms), Some(end_ms)) = (extent.start_ms, extent.end_ms) else {
            return Err(Problem::NoTimes);
        };
        let assets = assets(timeline, &files_in(timeline, extent), media)?;
        let mut all_cuts = estimate::stops(&doc.events);
        all_cuts.extend_from_slice(cuts);
        all_cuts.sort_by_key(|c| c.start_ms);
        let (pieces, cuts, cut_ms) = cut_pieces(timeline, &assets, (start_ms, end_ms), &all_cuts);
        if pieces.is_empty() {
            return Err(Problem::NotRecorded);
        }
        let (markers, unplaced) = place_markers(doc, extent, timeline, &assets, &pieces);
        Ok(Self {
            assets,
            pieces,
            markers,
            unplaced,
            cuts,
            cut_ms,
        })
    }

    /// タイムラインの長さ（フレームの数）。
    pub fn frames(&self) -> i64 {
        self.pieces.iter().map(Piece::frames).sum()
    }

    /// マーカーの数。
    pub fn marker_count(&self) -> usize {
        self.markers.len()
    }

    /// マーカーを、種類ごとに `leads` だけ前へずらす。タイムラインの頭より前にはしない。
    pub fn lead_markers(&mut self, leads: &MarkerLeads) {
        for marker in &mut self.markers {
            marker.frames = (marker.frames - ms_to_frames(leads.of(marker.kind))).max(0);
        }
        self.markers.sort_by_key(|m| m.frames);
    }

    /// FCPXML 1.10。`name` はタイムラインの名前になる。
    pub fn fcpxml(&self, name: &str) -> String {
        // 素材の大きさごとに format を 1 つ作る。番号は r2 から。r1 はタイムライン。
        let mut sizes: Vec<(u32, u32)> = Vec::new();
        let mut format_of = Vec::with_capacity(self.assets.len());
        for asset in &self.assets {
            format_of.push(match sizes.iter().position(|s| *s == asset.size) {
                Some(index) => index + 2,
                None => {
                    sizes.push(asset.size);
                    sizes.len() + 1
                }
            });
        }
        let format = |id: usize, (width, height): (u32, u32)| {
            format!(
                "    <format id=\"r{id}\" name=\"FFVideoFormat{height}p5994\" frameDuration=\"{FRAME}\" width=\"{width}\" height=\"{height}\"/>\n"
            )
        };

        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n<fcpxml version=\"1.10\">\n  <resources>\n",
        );
        xml.push_str(&format(1, SEQUENCE_SIZE));
        for (index, &size) in sizes.iter().enumerate() {
            xml.push_str(&format(index + 2, size));
        }
        for (index, asset) in self.assets.iter().enumerate() {
            let audio = if asset.audio_channels > 0 {
                format!(
                    " hasAudio=\"1\" audioSources=\"1\" audioChannels=\"{}\" audioRate=\"48000\"",
                    asset.audio_channels
                )
            } else {
                String::new()
            };
            xml.push_str(&format!(
                "    <asset id=\"a{index}\" name=\"{}\" start=\"{}\" duration=\"{}\" hasVideo=\"1\" format=\"r{}\"{audio}>\n      <media-rep kind=\"original-media\" src=\"{}\"/>\n    </asset>\n",
                escape(&asset.name),
                time(asset.start_frames),
                time(asset.frames),
                format_of[index],
                escape(&file_url(&asset.path)),
            ));
        }
        xml.push_str("  </resources>\n  <library>\n    <event name=\"routelog\">\n");
        // 素材より大きいタイムラインに、素材を引き伸ばして収める。
        xml.push_str(&format!(
            "      <project name=\"{}\">\n        <sequence format=\"r1\" duration=\"{}\" tcStart=\"{}\" tcFormat=\"DF\" audioLayout=\"stereo\" audioRate=\"48k\">\n          <spine>\n",
            escape(name),
            time(self.frames()),
            time(START_FRAMES),
        ));
        // 置く位置は、タイムラインのタイムコードで数える。頭は tcStart である。
        let mut offset = START_FRAMES;
        for piece in &self.pieces {
            let asset = &self.assets[piece.asset];
            xml.push_str(&format!(
                "            <asset-clip ref=\"a{}\" name=\"{}\" offset=\"{}\" start=\"{}\" duration=\"{}\" tcFormat=\"DF\">\n",
                piece.asset,
                escape(&asset.name),
                time(offset),
                time(asset.start_frames + piece.in_frames),
                time(piece.frames()),
            ));
            // 音はモノラルで録っている。1 チャンネル目を、左と右の両方に割り当てる。1 つの
            // audio-channel-source に「1, 1」と書くと、Resolve はモノラルとして読み、左からしか出ない。
            if asset.audio_channels > 0 {
                xml.push_str(
                    "              <audio-channel-source srcCh=\"1\" outCh=\"L\"/>\n              <audio-channel-source srcCh=\"1\" outCh=\"R\"/>\n",
                );
            }
            xml.push_str("            </asset-clip>\n");
            offset += piece.frames();
        }
        xml.push_str(
            "          </spine>\n        </sequence>\n      </project>\n    </event>\n  </library>\n</fcpxml>\n",
        );
        xml
    }

    /// マーカーを、Resolve の「EDL からタイムラインマーカー」で読み込む EDL にする。
    ///
    /// FCPXML のクリップに付けたマーカーは、Resolve が読まない。
    pub fn marker_edl(&self, title: &str) -> String {
        let mut edl = format!("TITLE: {}\nFCM: DROP FRAME\n\n", one_line(title));
        for (n, marker) in self.markers.iter().enumerate() {
            let at = START_FRAMES + marker.frames;
            let (start, end) = (frames_to_tc(at, 60), frames_to_tc(at + 1, 60));
            edl.push_str(&format!(
                "{:03}  001      V     C        {start} {end} {start} {end}  \n |C:{} |M:{} |D:1\n\n",
                n + 1,
                color(marker.kind),
                one_line(&marker.name).replace('|', "/"),
            ));
        }
        edl
    }
}

/// 改行を空白にする。EDL は 1 行ごとに読む。
fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// 書き出すファイルの名前の頭。「走行の題 回の名前」で、ファイル名に使えない字は `_` にする。
pub fn file_stem(ride_title: &str, episode_name: &str) -> String {
    let stem: String = format!("{} {}", ride_title.trim(), episode_name.trim())
        .chars()
        .map(|c| {
            if c.is_control() || "\\/:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Windows は、名前の終わりの点と空白を落とす。
    stem.trim().trim_end_matches(['.', ' ']).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episode::Episode;
    use crate::event::{Event, EventDetail, EventStatus};
    use crate::ride::{Ride, Route, SegmentMode, Waypoint};

    #[test]
    fn drop_frame_timecode_goes_both_ways() {
        assert_eq!(tc_to_frames("01:00:00;00", 60), Some(START_FRAMES));
        assert_eq!(frames_to_tc(START_FRAMES, 60), "01:00:00;00");
        // 1 分ごとに、頭の 4 フレームを飛ばす。10 分ごとは飛ばさない。
        assert_eq!(frames_to_tc(3599, 60), "00:00:59;59");
        assert_eq!(frames_to_tc(3600, 60), "00:01:00;04");
        assert_eq!(tc_to_frames("00:01:00;04", 60), Some(3600));
        assert_eq!(frames_to_tc(35_964, 60), "00:10:00;00");
        for frames in [
            0,
            1,
            3596,
            3600,
            35_963,
            35_964,
            1_000_000,
            START_FRAMES + 12_345,
        ] {
            assert_eq!(tc_to_frames(&frames_to_tc(frames, 60), 60), Some(frames));
        }
        assert_eq!(tc_to_frames("05:05:26", 30), None);
        assert_eq!(tc_to_frames("05:05:26;30", 30), None);
    }

    #[test]
    fn the_camera_timecode_counts_at_29_97() {
        // 録り始めの時刻より 26 秒ほど進んだタイムコードが、29.97 fps で付く。
        let frames = start_frames("05:05:26;07").unwrap();
        assert_eq!(frames % 2, 0);
        assert_eq!(frames_to_tc(frames, 60), "05:05:26;14");
    }

    #[test]
    fn milliseconds_round_to_the_nearest_frame() {
        assert_eq!(ms_to_frames(0), 0);
        assert_eq!(ms_to_frames(1001), 60);
        assert_eq!(ms_to_frames(1000), 60);
        assert_eq!(ms_to_frames(60_000), 3596);
        assert_eq!(frames_to_ms(60), 1001);
    }

    #[test]
    fn a_file_url_encodes_spaces_and_japanese() {
        assert_eq!(
            file_url(r"X:\例\サンプル 1\A.MP4"),
            "file:///X:/%E4%BE%8B/%E3%82%B5%E3%83%B3%E3%83%97%E3%83%AB%201/A.MP4"
        );
        assert_eq!(file_url("/example/a b.mp4"), "file:///example/a%20b.mp4");
    }

    #[test]
    fn the_file_stem_avoids_characters_windows_refuses() {
        assert_eq!(file_stem("御嶽山", "パート 1"), "御嶽山 パート 1");
        assert_eq!(file_stem("a/b", "c:d?"), "a_b c_d_");
        assert_eq!(file_stem("御嶽山", "前半."), "御嶽山 前半");
    }

    #[test]
    fn the_frame_rate_is_shown_short() {
        assert_eq!(fps(30000, 1001), "29.97");
        assert_eq!(fps(60000, 1001), "59.94");
        assert_eq!(fps(30, 1), "30");
        assert_eq!(fps(0, 0), "不明");
    }

    // ここから、回をタイムラインにする所。

    /// 2026-10-03 06:00:00 からの秒を、実時刻（ミリ秒）にする。
    fn at(seconds: i64) -> i64 {
        crate::jst::to_unix_ms(&crate::jst::Civil {
            year: 2026,
            month: 10,
            day: 3,
            hour: 6,
            minute: 0,
            second: 0,
            millisecond: 0,
        })
        .unwrap()
            + seconds * 1000
    }

    /// 0〜600 秒と、1000〜1600 秒を録画した 2 本。
    fn timeline() -> Timeline {
        let file = |name: &str, start: i64| VideoFile {
            path: format!(r"X:\example\{name}"),
            name: name.into(),
            seq: 0,
            start_ms: at(start),
            duration_ms: 600_000,
            codec: "hevc".into(),
        };
        Timeline::new(vec![file("A.MP4", 0), file("B.MP4", 1000)], 0)
    }

    fn media(timecode: &str) -> Option<Media> {
        Some(Media {
            frame_rate: FRAME_RATE,
            frames: ms_to_frames(600_000),
            width: 1920,
            height: 1080,
            audio_channels: 1,
            timecode: Some(timecode.into()),
        })
    }

    fn both() -> Vec<Option<Media>> {
        vec![media("06:00:26;00"), media("06:17:05;00")]
    }

    fn event(kind: EventKind, label: &str, route_m: f64, time_ms: Option<i64>) -> Event {
        Event {
            id: format!("{route_m}"),
            kind,
            label: label.into(),
            route_m,
            time_ms,
            status: EventStatus::Confirmed,
            hidden: false,
            detail: EventDetail::default(),
        }
    }

    fn doc(events: Vec<Event>) -> RideDocument {
        RideDocument {
            ride: Ride {
                id: "r".into(),
                title: "t".into(),
                ride_date: "2026-10-03".into(),
                clock_offset_ms: 0,
                is_public: false,
                labels: Vec::new(),
                created_at: "a".into(),
                updated_at: "b".into(),
            },
            waypoints: (0..2)
                .map(|i| Waypoint {
                    id: format!("w{i}"),
                    lon: 138.0 + f64::from(i) * 0.1,
                    lat: 36.0,
                    segment_mode: SegmentMode::Straight,
                })
                .collect(),
            route: Route {
                segments: vec![Vec::new()],
            },
            events,
            episodes: vec![Episode {
                id: "e".into(),
                title: String::new(),
                from: "w0".into(),
                to: "w1".into(),
            }],
        }
    }

    /// 経路の 0〜10 km を、`start`〜`end` 秒に通った回。
    fn extent(start: i64, end: i64) -> Extent {
        Extent {
            from: 0,
            to: 1,
            start_m: 0.0,
            end_m: 10_000.0,
            start_ms: Some(at(start)),
            end_ms: Some(at(end)),
        }
    }

    fn piece(asset: usize, in_s: i64, out_s: i64) -> Piece {
        Piece {
            asset,
            in_frames: ms_to_frames(in_s * 1000),
            out_frames: ms_to_frames(out_s * 1000),
        }
    }

    fn cut(start: i64, end: i64) -> Stop {
        Stop {
            start_ms: at(start),
            end_ms: at(end),
        }
    }

    #[test]
    fn the_episode_cuts_the_recording_at_its_ends() {
        let made = SourceTimeline::new(
            &doc(Vec::new()),
            &extent(100, 1200),
            &timeline(),
            &both(),
            &[],
        )
        .unwrap();
        assert_eq!(made.pieces, [piece(0, 100, 600), piece(1, 0, 200)]);
        assert_eq!(made.frames(), ms_to_frames(500_000) + ms_to_frames(200_000));
        assert_eq!((made.cuts, made.cut_ms), (0, 0));
    }

    #[test]
    fn candidates_in_a_file_become_real_time_cut_where_the_next_file_starts() {
        // A は 0〜700 秒の長さで、B が始まる 650 秒で切ってある。
        let file = |name: &str, start: i64, length: i64| VideoFile {
            path: name.into(),
            name: name.into(),
            seq: 0,
            start_ms: at(start),
            duration_ms: length * 1000,
            codec: "hevc".into(),
        };
        let overlapping = Timeline::new(vec![file("A", 0, 700), file("B", 650, 100)], 0);
        let cuts = real_cuts(&overlapping, |f| match f.name.as_str() {
            "A" => vec![(10_000, 20_000), (640_000, 690_000), (660_000, 680_000)],
            _ => vec![(5_000, 8_000)],
        });
        assert_eq!(cuts, [cut(10, 20), cut(640, 650), cut(655, 658)]);
    }

    #[test]
    fn stops_and_taken_candidates_are_cut_where_recorded() {
        let mut stop = event(EventKind::Stop, "", 1000.0, Some(at(200)));
        stop.detail.stay_ms = Some(60_000);
        // 録画を止めた間の停車は、何も抜かない。
        let mut idle = event(EventKind::Stop, "", 2000.0, Some(at(700)));
        idle.detail.stay_ms = Some(100_000);
        let made = SourceTimeline::new(
            &doc(vec![stop, idle]),
            &extent(0, 1600),
            &timeline(),
            &both(),
            &[cut(1100, 1130), cut(550, 1050)],
        )
        .unwrap();
        assert_eq!(
            made.pieces,
            [
                piece(0, 0, 200),
                piece(0, 260, 550),
                piece(1, 50, 100),
                piece(1, 130, 600)
            ]
        );
        assert_eq!(made.cuts, 4, "録画をまたぐ候補は、両方のファイルで数える");
        assert_eq!(made.cut_ms, (60 + 50 + 50 + 30) * 1000);
    }

    #[test]
    fn markers_sit_on_the_timeline_after_the_cuts() {
        let mut stop = event(EventKind::Stop, "", 100.0, Some(at(100)));
        stop.detail.stay_ms = Some(50_000);
        let mut hidden = event(EventKind::Bridge, "隠した橋", 500.0, Some(at(300)));
        hidden.hidden = true;
        let events = vec![
            event(EventKind::Departure, "出発地", 0.0, Some(at(10))),
            stop,
            event(EventKind::Tunnel, "", 200.0, Some(at(120))),
            hidden,
            event(EventKind::Road, "国道19号", 600.0, Some(at(400))),
            event(EventKind::Bridge, "止めた間の橋", 700.0, Some(at(800))),
            event(EventKind::MichiNoEki, "時刻の無い道の駅", 800.0, None),
            event(EventKind::Bridge, "次の橋", 900.0, Some(at(1010))),
            event(EventKind::Bridge, "範囲の外の橋", 20_000.0, Some(at(1100))),
        ];
        let made =
            SourceTimeline::new(&doc(events), &extent(0, 1600), &timeline(), &both(), &[]).unwrap();
        let shown: Vec<(i64, &str)> = made
            .markers
            .iter()
            .map(|m| (m.frames, m.name.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                (ms_to_frames(10_000), "出発地"),
                (ms_to_frames(400_000) - ms_to_frames(50_000), "国道19号"),
                (
                    ms_to_frames(600_000) - ms_to_frames(50_000) + ms_to_frames(10_000),
                    "次の橋"
                ),
            ]
        );
        // 停車そのものと、停車の中のトンネル、止めた間の橋、時刻の無い道の駅。
        assert_eq!(made.unplaced, 4);
        // 名前の無いイベントは、種類で呼ぶ。
        let tunnel = vec![event(EventKind::Tunnel, " ", 200.0, Some(at(120)))];
        let made =
            SourceTimeline::new(&doc(tunnel), &extent(0, 1600), &timeline(), &both(), &[]).unwrap();
        assert_eq!(made.markers[0].name, "トンネル");
    }

    #[test]
    fn markers_lead_by_kind_but_not_before_the_timeline() {
        let events = vec![
            event(EventKind::Departure, "出発地", 0.0, Some(at(1))),
            event(EventKind::Road, "国道19号", 600.0, Some(at(400))),
            event(EventKind::Bridge, "橋", 610.0, Some(at(401))),
        ];
        let mut made =
            SourceTimeline::new(&doc(events), &extent(0, 1600), &timeline(), &both(), &[]).unwrap();
        made.lead_markers(&MarkerLeads {
            road: 500,
            bridge: 3000,
            ..MarkerLeads::default()
        });
        let shown: Vec<(i64, &str)> = made
            .markers
            .iter()
            .map(|m| (m.frames, m.name.as_str()))
            .collect();
        // 出発は 1 秒の所にあり、2 秒前は頭より前になる。橋は路線より後を通るが、前に来る。
        assert_eq!(
            shown,
            [
                (0, "出発地"),
                (ms_to_frames(401_000) - ms_to_frames(3000), "橋"),
                (ms_to_frames(400_000) - ms_to_frames(500), "国道19号"),
            ]
        );
    }

    #[test]
    fn the_marker_leads_read_from_an_older_config_are_two_seconds() {
        let leads: MarkerLeads = serde_json::from_str(r#"{"road": 1500}"#).unwrap();
        assert_eq!(leads.road, 1500);
        assert_eq!(leads.tunnel, 2000);
    }

    #[test]
    fn what_cannot_be_exported_says_why() {
        let d = doc(Vec::new());
        let mut unknown = extent(0, 1600);
        unknown.end_ms = None;
        assert_eq!(
            SourceTimeline::new(&d, &unknown, &timeline(), &both(), &[]),
            Err(Problem::NoTimes)
        );
        assert_eq!(
            SourceTimeline::new(&d, &extent(650, 950), &timeline(), &both(), &[]),
            Err(Problem::NotRecorded)
        );
        assert_eq!(
            SourceTimeline::new(&d, &extent(0, 500), &timeline(), &both(), &[cut(0, 500)]),
            Err(Problem::NotRecorded)
        );

        let mut slow = both();
        slow[1].as_mut().unwrap().frame_rate = (30000, 1001);
        let problem = SourceTimeline::new(&d, &extent(0, 1600), &timeline(), &slow, &[]);
        assert_eq!(
            problem,
            Err(Problem::FrameRate(vec![("B.MP4".into(), (30000, 1001))]))
        );
        assert!(
            problem
                .unwrap_err()
                .to_string()
                .contains("B.MP4（29.97 fps）")
        );
        // 範囲の外の動画は、問わない。
        assert!(SourceTimeline::new(&d, &extent(0, 500), &timeline(), &slow, &[]).is_ok());

        let mut untimed = both();
        untimed[0].as_mut().unwrap().timecode = None;
        assert_eq!(
            SourceTimeline::new(&d, &extent(0, 1600), &timeline(), &untimed, &[]),
            Err(Problem::NoTimecode(vec!["A.MP4".into()]))
        );
        assert_eq!(files_in(&timeline(), &extent(0, 500)), [0]);
        assert_eq!(files_in(&timeline(), &unknown), Vec::<usize>::new());
    }

    #[test]
    fn the_fcpxml_points_at_the_source_timecode() {
        let made = SourceTimeline::new(
            &doc(Vec::new()),
            &extent(100, 1200),
            &timeline(),
            &both(),
            &[],
        )
        .unwrap();
        let xml = made.fcpxml("御嶽山 パート 1 ソース");
        let a_start = start_frames("06:00:26;00").unwrap();
        let b_start = start_frames("06:17:05;00").unwrap();
        let first = ms_to_frames(500_000);
        for expected in [
            "<fcpxml version=\"1.10\">".to_owned(),
            "<format id=\"r1\" name=\"FFVideoFormat2160p5994\" frameDuration=\"1001/60000s\" width=\"3840\" height=\"2160\"/>".to_owned(),
            "<format id=\"r2\" name=\"FFVideoFormat1080p5994\" frameDuration=\"1001/60000s\" width=\"1920\" height=\"1080\"/>".to_owned(),
            format!("<asset id=\"a0\" name=\"A.MP4\" start=\"{}\" duration=\"{}\" hasVideo=\"1\" format=\"r2\" hasAudio=\"1\" audioSources=\"1\" audioChannels=\"1\" audioRate=\"48000\">", time(a_start), time(ms_to_frames(600_000))),
            "<media-rep kind=\"original-media\" src=\"file:///X:/example/A.MP4\"/>".to_owned(),
            "<project name=\"御嶽山 パート 1 ソース\">".to_owned(),
            format!("<sequence format=\"r1\" duration=\"{}\" tcStart=\"215999784/60000s\" tcFormat=\"DF\"", time(made.frames())),
            format!("<asset-clip ref=\"a0\" name=\"A.MP4\" offset=\"215999784/60000s\" start=\"{}\" duration=\"{}\" tcFormat=\"DF\">", time(a_start + ms_to_frames(100_000)), time(first)),
            format!("<asset-clip ref=\"a1\" name=\"B.MP4\" offset=\"{}\" start=\"{}\" duration=\"{}\" tcFormat=\"DF\">", time(START_FRAMES + first), time(b_start), time(ms_to_frames(200_000))),
            "<audio-channel-source srcCh=\"1\" outCh=\"L\"/>\n              <audio-channel-source srcCh=\"1\" outCh=\"R\"/>".to_owned(),
        ] {
            assert!(xml.contains(&expected), "{expected}\n---\n{xml}");
        }
        assert_eq!(xml.matches("<asset-clip").count(), 2);
    }

    #[test]
    fn a_silent_file_gets_no_audio_mapping() {
        let mut silent = both();
        silent[0].as_mut().unwrap().audio_channels = 0;
        let made =
            SourceTimeline::new(&doc(Vec::new()), &extent(0, 500), &timeline(), &silent, &[])
                .unwrap();
        let xml = made.fcpxml("t");
        assert!(!xml.contains("hasAudio"));
        assert!(!xml.contains("audio-channel-source"));
    }

    #[test]
    fn the_marker_edl_starts_at_one_hour() {
        let events = vec![
            event(EventKind::Departure, "出発地", 0.0, Some(at(10))),
            event(EventKind::Road, "国道19号|旧道\n", 600.0, Some(at(70))),
        ];
        let made =
            SourceTimeline::new(&doc(events), &extent(0, 1600), &timeline(), &both(), &[]).unwrap();
        let edl = made.marker_edl("御嶽山 パート 1 ソース");
        let ten = frames_to_tc(START_FRAMES + ms_to_frames(10_000), 60);
        let ten_next = frames_to_tc(START_FRAMES + ms_to_frames(10_000) + 1, 60);
        assert_eq!(
            ten, "01:00:09;59",
            "実時間の 10 秒は、タイムコードでは 10 秒に 1 フレーム足りない"
        );
        assert_eq!(
            edl,
            format!(
                "TITLE: 御嶽山 パート 1 ソース\nFCM: DROP FRAME\n\n\
                 001  001      V     C        {ten} {ten_next} {ten} {ten_next}  \n |C:ResolveColorRed |M:出発地 |D:1\n\n\
                 002  001      V     C        01:01:10;00 01:01:10;01 01:01:10;00 01:01:10;01  \n |C:ResolveColorBlue |M:国道19号/旧道 |D:1\n\n"
            )
        );
    }
}
