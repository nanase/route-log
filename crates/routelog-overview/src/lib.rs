//! 経路の概略図の動画。動画の冒頭で、これから走る経路を見せる。
//!
//! 背景は国土地理院の写真のタイルで、その上に経路の線と、始点・終点の印と、出典を描く。経路の
//! 全体を少し引いた所から見せ、ゆっくり寄って、全体が収まる所で止まり、しばらくそのまま見せる。
//! 回の動画では、全部の回を合わせた経路を見せてから、この回へ寄る。他の回は、寄るにつれて薄くする。

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
use view::{Flight, View, project};
pub use view::{Fps, Motion};

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

/// 描く経路。
#[derive(Clone, Copy)]
pub struct Route<'a> {
    /// はっきり描く経路。回か、経由点で選んだ範囲。両端に始点と終点の印を置く
    pub line: &'a [LonLat],
    /// 薄く描く経路。全部の回を合わせた範囲のうち、`line` の外の部分。最初の見え方は、これも
    /// 含めて収める。寄っていくにつれて薄くする
    pub others: &'a [Vec<LonLat>],
}

/// 世界座標にした経路と、最初の見え方から止まった所への道のり。
struct Scene {
    line: Vec<[f64; 2]>,
    others: Vec<Vec<[f64; 2]>>,
    flight: Flight,
}

impl Scene {
    /// 止まった所では `line` の全体が収まる。最初の見え方は、止まった所を `start_offset` だけ
    /// 引いた所。それでも全経路が収まらなければ、収まるまで引く。
    fn new(route: Route, size: [u32; 2], start_offset: f64) -> Result<Scene, Error> {
        let projected =
            |line: &[LonLat]| -> Vec<[f64; 2]> { line.iter().copied().map(project).collect() };
        let line = projected(route.line);
        let others: Vec<_> = route.others.iter().map(|l| projected(l)).collect();
        let stop = View::fit(&line, size).ok_or(Error::NoRoute)?;
        let all: Vec<[f64; 2]> = line
            .iter()
            .chain(others.iter().flatten())
            .copied()
            .collect();
        let whole = View::fit(&all, size).ok_or(Error::NoRoute)?;
        let from = View {
            center: whole.center,
            zoom: whole.zoom.min(stop.zoom + start_offset),
        };
        Ok(Scene {
            line,
            others,
            flight: Flight { from, to: stop },
        })
    }
}

/// 見本にする所。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Still {
    /// 寄り始める前の、最初の見え方。`scale` は、止まった所に対する最初の大きさ
    Start { scale: f64 },
    /// 止まった所
    Stop,
}

/// `still` の所の 1 枚を、`size` の大きさの PNG にする。書き出す前に見た目を確かめるのに使う。
pub fn preview(
    route: Route,
    still: Still,
    size: [u32; 2],
    sources: Sources,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, Error> {
    let start_offset = match still {
        Still::Start { scale } => scale.log2(),
        Still::Stop => 0.0,
    };
    let scene = Scene::new(route, size, start_offset)?;
    let progress = match still {
        Still::Start { .. } => 0.0,
        Still::Stop => 1.0,
    };
    let view = scene.flight.at(progress);
    let (photos, credit) = photos(sources, &Flight::still(view), size, &mut |_| {}, cancel)?;
    let mut pixmap = canvas(size);
    frame(&mut pixmap, &photos, &view, &scene, progress, credit);
    pixmap
        .encode_png()
        .map_err(|e| Error::Encode(e.to_string()))
}

/// 動画を `out` に書き出す。やめたときと失敗したときは、書きかけのファイルを消す。
pub fn render(
    route: Route,
    motion: Motion,
    sources: Sources,
    ffmpeg: &Ffmpeg,
    out: &Path,
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<(), Error> {
    let result = write(route, motion, sources, ffmpeg, out, progress, cancel);
    if result.is_err() {
        let _ = std::fs::remove_file(out);
    }
    result
}

fn write(
    route: Route,
    motion: Motion,
    sources: Sources,
    ffmpeg: &Ffmpeg,
    out: &Path,
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<(), Error> {
    let size = VIDEO_SIZE;
    let scene = Scene::new(route, size, motion.start_offset())?;
    let (photos, credit) = photos(sources, &scene.flight, size, progress, cancel)?;

    let total = motion.lead_frames() + motion.zoom_frames() + motion.hold_frames();
    let mut encoder = ffmpeg.start(
        out,
        size,
        motion.fps,
        motion.lead_frames(),
        motion.hold_frames(),
    )?;
    let mut pixmap = canvas(size);
    for index in 0..motion.zoom_frames() {
        if cancel.load(Ordering::Relaxed) {
            encoder.abort();
            return Err(Error::Cancelled);
        }
        let at = motion.progress(index);
        let view = scene.flight.at(at);
        frame(&mut pixmap, &photos, &view, &scene, at, credit);
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

/// `flight` の道のりの見え方を、すべて写せる写真と、それに合う出典。
fn photos<'a>(
    sources: Sources<'a>,
    flight: &Flight,
    size: [u32; 2],
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<(Pyramid, &'a Pixmap), Error> {
    let ranges = Pyramid::ranges(flight, size, flight.to.tile_zoom());
    let (mosaics, lowest) = sources.tiles.mosaics(&ranges, progress, cancel)?;
    Ok((
        Pyramid::new(mosaics, &flight.from),
        sources.credits.pick(lowest),
    ))
}

fn canvas([width, height]: [u32; 2]) -> Pixmap {
    Pixmap::new(width, height).expect("画面の大きさは 0 でない")
}

/// 1 枚の絵を描く。写真、薄く描く経路、はっきり描く経路、始点と終点の印、出典の順に重ねる。
/// `progress` は、道のりのどこを描くか。薄く描く経路は、進むにつれて薄くする。
fn frame(
    pixmap: &mut Pixmap,
    photos: &Pyramid,
    view: &View,
    scene: &Scene,
    progress: f64,
    credit: &Pixmap,
) {
    let size = [pixmap.width(), pixmap.height()];
    photos.draw(view, pixmap);
    let screen = |points: &[[f64; 2]]| -> Vec<[f32; 2]> {
        points
            .iter()
            .map(|p| view.on_screen(*p, size).map(|v| v as f32))
            .collect()
    };
    let others: Vec<_> = scene.others.iter().map(|l| screen(l)).collect();
    draw::faded_routes(pixmap, &others, draw::fade(progress));
    let line = screen(&scene.line);
    draw::route(pixmap, &line);
    if let (Some(first), Some(last)) = (line.first(), line.last()) {
        draw::start(pixmap, *first);
        draw::finish(pixmap, *last);
    }
    draw::credit(pixmap, credit);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 経度で 0.5 度ずつ東へ進む 3 点。前半と後半に分ける。
    const WHOLE: [LonLat; 3] = [[138.0, 36.0], [138.5, 36.0], [139.0, 36.0]];

    #[test]
    fn without_other_episodes_the_start_is_the_stop_drawn_back() {
        let line = WHOLE.to_vec();
        let route = Route {
            line: &line,
            others: &[],
        };
        let Flight { from, to } = Scene::new(route, VIDEO_SIZE, -2.0).unwrap().flight;
        assert_eq!((from.center, from.zoom), (to.center, to.zoom - 2.0));
    }

    #[test]
    fn the_start_shows_every_episode_and_draws_back_at_least_by_the_start_scale() {
        let (line, others) = (WHOLE[..2].to_vec(), vec![WHOLE[1..].to_vec()]);
        let route = Route {
            line: &line,
            others: &others,
        };
        let points: Vec<_> = WHOLE.iter().copied().map(project).collect();
        let whole = View::fit(&points, VIDEO_SIZE).unwrap();
        // 前半は全経路の半分の幅なので、全部が収まるのは止まった所から 1 段引いた所。
        let Flight { from, to } = Scene::new(route, VIDEO_SIZE, 0.0).unwrap().flight;
        assert_eq!(from, whole);
        assert!((to.zoom - whole.zoom - 1.0).abs() < 1e-9);
        // 最初の大きさが 4 分の 1 なら、全経路の真ん中を中心に、止まった所から 2 段引く。
        let Flight { from, to } = Scene::new(route, VIDEO_SIZE, -2.0).unwrap().flight;
        assert_eq!(from.center, whole.center);
        assert!((from.zoom - (to.zoom - 2.0)).abs() < 1e-9);
    }
}
