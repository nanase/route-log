//! 経路の概略図の動画。動画の冒頭で、これから走る経路を見せる。
//!
//! 背景は国土地理院の写真のタイルで、その上に経路の線と、始点・終点の印と、出典を描く。経路の
//! 全体を少し引いた所から見せ、ゆっくり寄って、全体が収まる所で止まり、しばらくそのまま見せる。

mod draw;
mod encode;
mod mosaic;
mod tiles;
mod view;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use tiny_skia::Pixmap;

pub use encode::{Codec, Ffmpeg};
use mosaic::Pyramid;
pub use tiles::TileStore;
pub use view::{Fps, Motion};
use view::{View, project};

/// 経度と緯度。
pub type LonLat = [f64; 2];

/// 書き出す動画の大きさ（4K）。
pub const VIDEO_SIZE: [u32; 2] = [3840, 2160];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("経路がありません")]
    NoRoute,
    #[error("写真のタイルを取れません: {0}")]
    Tile(String),
    #[error("出典の画像を読めません: {0}")]
    Credit(String),
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("{program} を起動できません: {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    #[error("動画を書けません: {0}")]
    Encode(String),
    #[error("やめました")]
    Cancelled,
}

/// 進み具合。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// 写真のタイルを取っている
    Tiles { done: usize, total: usize },
    /// 動画を書いている
    Frames { done: u32, total: u32 },
}

/// 右下に置く出典。幅 3840 px の画面に合わせて作った画像を、使った写真に合わせて選ぶ。
pub struct Credits {
    /// 全国最新写真だけを使ったとき
    pub photo: Pixmap,
    /// 衛星画像（Landsat 8）のタイルも使ったとき
    pub landsat: Pixmap,
}

impl Credits {
    pub fn from_png(photo: &[u8], landsat: &[u8]) -> Result<Credits, Error> {
        let decode = |png: &[u8]| Pixmap::decode_png(png).map_err(|e| Error::Credit(e.to_string()));
        Ok(Credits {
            photo: decode(photo)?,
            landsat: decode(landsat)?,
        })
    }

    /// 使ったタイルのうち、いちばん低いズームに合う出典。
    fn pick(&self, lowest_zoom: u8) -> &Pixmap {
        if lowest_zoom <= tiles::LANDSAT_MAX_ZOOM {
            &self.landsat
        } else {
            &self.photo
        }
    }
}

/// 絵の材料。背景の写真のタイルと、出典の画像。
#[derive(Clone, Copy)]
pub struct Sources<'a> {
    pub tiles: &'a TileStore,
    pub credits: &'a Credits,
}

/// 止まった所の 1 枚を、`size` の大きさの PNG にする。書き出す前に見た目を確かめるのに使う。
pub fn preview(
    line: &[LonLat],
    size: [u32; 2],
    sources: Sources,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, Error> {
    let points: Vec<[f64; 2]> = line.iter().copied().map(project).collect();
    let view = View::fit(&points, size).ok_or(Error::NoRoute)?;
    let (photos, credit) = photos(sources, &view, &view, size, &mut |_| {}, cancel)?;
    let mut pixmap = canvas(size);
    frame(&mut pixmap, &photos, &view, &points, credit);
    pixmap
        .encode_png()
        .map_err(|e| Error::Encode(e.to_string()))
}

/// 動画を `out` に書き出す。やめたときと失敗したときは、書きかけのファイルを消す。
pub fn render(
    line: &[LonLat],
    motion: Motion,
    sources: Sources,
    ffmpeg: &Ffmpeg,
    out: &Path,
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<(), Error> {
    let result = write(line, motion, sources, ffmpeg, out, progress, cancel);
    if result.is_err() {
        let _ = std::fs::remove_file(out);
    }
    result
}

fn write(
    line: &[LonLat],
    motion: Motion,
    sources: Sources,
    ffmpeg: &Ffmpeg,
    out: &Path,
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<(), Error> {
    let size = VIDEO_SIZE;
    let points: Vec<[f64; 2]> = line.iter().copied().map(project).collect();
    let stop = View::fit(&points, size).ok_or(Error::NoRoute)?;
    // 最初がいちばん広く見えるので、そこまでの写真を敷いておく。
    let first = View {
        zoom: stop.zoom + motion.start_offset(),
        ..stop
    };
    let (photos, credit) = photos(sources, &stop, &first, size, progress, cancel)?;

    let total = motion.zoom_frames() + motion.hold_frames();
    let mut encoder = ffmpeg.start(out, size, motion.fps, motion.hold_frames())?;
    let mut pixmap = canvas(size);
    for index in 0..motion.zoom_frames() {
        if cancel.load(Ordering::Relaxed) {
            encoder.abort();
            return Err(Error::Cancelled);
        }
        let view = View {
            zoom: stop.zoom + motion.offset(index),
            ..stop
        };
        frame(&mut pixmap, &photos, &view, &points, credit);
        encoder.write(pixmap.data())?;
        progress(Progress::Frames {
            done: encoder.encoded(),
            total,
        });
    }
    encoder.finish(
        &mut |done| progress(Progress::Frames { done, total }),
        &|| cancel.load(Ordering::Relaxed),
    )
}

/// `stop` から `widest` まで引いた所までを写せる写真と、それに合う出典。
fn photos<'a>(
    sources: Sources<'a>,
    stop: &View,
    widest: &View,
    size: [u32; 2],
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<(Pyramid, &'a Pixmap), Error> {
    let ranges = Pyramid::ranges(stop, widest, size, stop.tile_zoom());
    let (mosaics, lowest) = sources.tiles.mosaics(&ranges, progress, cancel)?;
    Ok((Pyramid::new(mosaics, widest), sources.credits.pick(lowest)))
}

fn canvas([width, height]: [u32; 2]) -> Pixmap {
    Pixmap::new(width, height).expect("画面の大きさは 0 でない")
}

/// 1 枚の絵を描く。写真、経路の線、始点と終点の印、出典の順に重ねる。
fn frame(pixmap: &mut Pixmap, photos: &Pyramid, view: &View, points: &[[f64; 2]], credit: &Pixmap) {
    let size = [pixmap.width(), pixmap.height()];
    photos.draw(view, pixmap);
    let screen: Vec<[f32; 2]> = points
        .iter()
        .map(|p| view.on_screen(*p, size).map(|v| v as f32))
        .collect();
    draw::route(pixmap, &screen);
    if let (Some(first), Some(last)) = (screen.first(), screen.last()) {
        draw::start(pixmap, *first);
        draw::finish(pixmap, *last);
    }
    draw::credit(pixmap, credit);
}
