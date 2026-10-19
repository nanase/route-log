//! 国土地理院の写真のタイルを取ってくる。取ったものは手元に置き、次からはそれを使う。
//!
//! 海の上などで写真の無いタイルは、ズームを下げたタイルの一部を引き伸ばして埋める。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use crate::mosaic::{Mosaic, TILE, TILE_BYTES};
use crate::view::{MIN_TILE_ZOOM, TileRange};
use crate::{Error, Progress};

const URL: &str = "https://cyberjapandata.gsi.go.jp/xyz/seamlessphoto";
/// 同時に取りに行く数。
const WORKERS: usize = 6;
/// 写真の無いタイルを埋めるのに、ズームをいくつまで下げるか。
const FALLBACK_LEVELS: u8 = 5;
/// これ以下のズームは、全国最新写真ではなく、衛星画像（Landsat 8）でできている。
pub const LANDSAT_MAX_ZOOM: u8 = 13;

/// RGB の 256 px 四方。
type Rgb = Vec<u8>;

pub struct TileStore {
    dir: PathBuf,
    agent: ureq::Agent,
}

impl TileStore {
    /// `dir` に、取ったタイルを置く。
    pub fn new(dir: impl Into<PathBuf>) -> TileStore {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("route-log/", env!("CARGO_PKG_VERSION")))
            .build();
        TileStore {
            dir: dir.into(),
            agent,
        }
    }

    /// `ranges` のそれぞれのタイルを敷いた写真と、使ったタイルのうちいちばん低いズーム。
    pub fn mosaics(
        &self,
        ranges: &[TileRange],
        progress: &mut dyn FnMut(Progress),
        cancel: &AtomicBool,
    ) -> Result<(Vec<Mosaic>, u8), Error> {
        let tiles: Vec<(usize, u32, u32)> = ranges
            .iter()
            .enumerate()
            .flat_map(|(i, range)| range.tiles().map(move |(x, y)| (i, x, y)))
            .collect();
        let total = tiles.len();
        let next = AtomicUsize::new(0);
        // 失敗したら、残りを取りに行かせない。
        let stop = AtomicBool::new(false);
        let mut mosaics: Vec<Mosaic> = ranges.iter().copied().map(Mosaic::new).collect();
        let mut lowest = ranges.iter().map(|r| r.zoom).min().unwrap_or(u8::MAX);
        progress(Progress::Tiles { done: 0, total });
        std::thread::scope(|scope| {
            let (sender, receiver) = mpsc::channel();
            for _ in 0..WORKERS.min(total) {
                let sender = sender.clone();
                let (tiles, next, stop) = (&tiles, &next, &stop);
                scope.spawn(move || {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= tiles.len()
                            || stop.load(Ordering::Relaxed)
                            || cancel.load(Ordering::Relaxed)
                        {
                            break;
                        }
                        let (i, x, y) = tiles[index];
                        let found = self.filled(ranges[i].zoom, x, y);
                        let failed = found.is_err();
                        if sender.send((i, x, y, found)).is_err() || failed {
                            break;
                        }
                    }
                });
            }
            drop(sender);
            for (index, (i, x, y, found)) in receiver.into_iter().enumerate() {
                let (rgb, zoom) = found.inspect_err(|_| stop.store(true, Ordering::Relaxed))?;
                if let Some(rgb) = rgb {
                    mosaics[i].put(x, y, &rgb);
                }
                lowest = lowest.min(zoom);
                progress(Progress::Tiles {
                    done: index + 1,
                    total,
                });
            }
            Ok::<_, Error>(())
        })?;
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        Ok((mosaics, lowest))
    }

    /// タイル 1 枚。写真が無ければ、ズームを下げたタイルの一部を引き伸ばす。それも無ければ None。
    /// 使ったタイルのズームも返す。
    fn filled(&self, zoom: u8, x: u32, y: u32) -> Result<(Option<Rgb>, u8), Error> {
        if let Some(rgb) = self.tile(zoom, x, y)? {
            return Ok((Some(rgb), zoom));
        }
        for down in 1..=FALLBACK_LEVELS {
            if zoom < MIN_TILE_ZOOM + down {
                break;
            }
            if let Some(parent) = self.tile(zoom - down, x >> down, y >> down)? {
                return Ok((Some(enlarge(&parent, down, x, y)), zoom - down));
            }
        }
        Ok((None, zoom))
    }

    /// タイル 1 枚。手元に無ければ取りに行く。写真の無いタイルは None。
    fn tile(&self, zoom: u8, x: u32, y: u32) -> Result<Option<Rgb>, Error> {
        let path = self.dir.join(zoom.to_string()).join(x.to_string());
        let (jpeg, none) = (
            path.join(format!("{y}.jpg")),
            path.join(format!("{y}.none")),
        );
        if none.exists() {
            return Ok(None);
        }
        let bytes = match fs::read(&jpeg) {
            Ok(bytes) => bytes,
            Err(_) => match self.download(zoom, x, y)? {
                Some(bytes) => {
                    write(&jpeg, &bytes)?;
                    bytes
                }
                None => {
                    write(&none, &[])?;
                    return Ok(None);
                }
            },
        };
        decode(&bytes)
            .map(Some)
            .map_err(|e| Error::Tile(format!("{zoom}/{x}/{y}: {e}")))
    }

    /// タイルを取りに行く。写真の無いタイル（404）は None。通信の失敗は 1 度だけやり直す。
    fn download(&self, zoom: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>, Error> {
        let url = format!("{URL}/{zoom}/{x}/{y}.jpg");
        let mut tries = 0;
        loop {
            tries += 1;
            match self.agent.get(&url).call() {
                Ok(response) => {
                    let mut bytes = Vec::new();
                    std::io::Read::read_to_end(&mut response.into_reader(), &mut bytes)
                        .map_err(|e| Error::Tile(format!("{url}: {e}")))?;
                    return Ok(Some(bytes));
                }
                Err(ureq::Error::Status(404, _)) => return Ok(None),
                Err(ureq::Error::Transport(_)) if tries < 2 => continue,
                Err(e) => return Err(Error::Tile(format!("{url}: {e}"))),
            }
        }
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let io = |source| Error::Io {
        path: path.display().to_string(),
        source,
    };
    fs::create_dir_all(path.parent().expect("タイルの置き場所はフォルダの下にある")).map_err(io)?;
    fs::write(path, bytes).map_err(io)
}

fn decode(bytes: &[u8]) -> Result<Rgb, image::ImageError> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)?.to_rgb8();
    let rgb = image.into_raw();
    if rgb.len() != TILE_BYTES {
        return Err(image::ImageError::Limits(
            image::error::LimitError::from_kind(image::error::LimitErrorKind::DimensionError),
        ));
    }
    Ok(rgb)
}

/// ズームを `down` 下げたタイルのうち、タイル `(x, y)` に当たる所を、256 px 四方に引き伸ばす。
fn enlarge(parent: &[u8], down: u8, x: u32, y: u32) -> Rgb {
    let side = (TILE >> down) as f32;
    let mask = (1u32 << down) - 1;
    let (left, top) = ((x & mask) as f32 * side, (y & mask) as f32 * side);
    let step = side / TILE as f32;
    let at = |px: usize, py: usize, c: usize| f32::from(parent[(py * TILE + px) * 3 + c]);
    let mut rgb = vec![0; TILE_BYTES];
    for row in 0..TILE {
        let fy = (top + (row as f32 + 0.5) * step - 0.5).clamp(0.0, (TILE - 1) as f32);
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(TILE - 1);
        for column in 0..TILE {
            let fx = (left + (column as f32 + 0.5) * step - 0.5).clamp(0.0, (TILE - 1) as f32);
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let x1 = (x0 + 1).min(TILE - 1);
            for c in 0..3 {
                let top = at(x0, y0, c) + (at(x1, y0, c) - at(x0, y0, c)) * tx;
                let bottom = at(x0, y1, c) + (at(x1, y1, c) - at(x0, y1, c)) * tx;
                rgb[(row * TILE + column) * 3 + c] = (top + (bottom - top) * ty + 0.5) as u8;
            }
        }
    }
    rgb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_tile_is_filled_from_the_right_quarter_of_its_parent() {
        // 左上が 0、右上が 100、左下が 200、右下が 250 の親から、右下の子を作る。
        let mut parent = vec![0; TILE_BYTES];
        for y in 0..TILE {
            for x in 0..TILE {
                let value = match (x < TILE / 2, y < TILE / 2) {
                    (true, true) => 0,
                    (false, true) => 100,
                    (true, false) => 200,
                    (false, false) => 250,
                };
                parent[(y * TILE + x) * 3..(y * TILE + x) * 3 + 3].fill(value);
            }
        }
        let child = enlarge(&parent, 1, 7, 9);
        assert_eq!(child[(128 * TILE + 128) * 3], 250);
        let left = enlarge(&parent, 1, 6, 8);
        assert_eq!(left[(128 * TILE + 128) * 3], 0);
    }

    #[test]
    fn stored_tiles_and_missing_marks_are_used_without_downloading() {
        let dir = std::env::temp_dir().join(format!("routelog-tiles-{}", std::process::id()));
        let image = image::RgbImage::from_pixel(256, 256, image::Rgb([10, 200, 30]));
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgb8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        write(&dir.join("12/100/200.jpg"), &jpeg).unwrap();
        write(&dir.join("13/200/400.none"), &[]).unwrap();
        let store = TileStore::new(&dir);
        let rgb = store.tile(12, 100, 200).unwrap().unwrap();
        assert!(rgb[0].abs_diff(10) < 4 && rgb[1].abs_diff(200) < 4);
        assert!(store.tile(13, 200, 400).unwrap().is_none());
        // 写真の無いタイルは、ズームを 1 つ下げたタイルから埋める。
        let (filled, zoom) = store.filled(13, 200, 400).unwrap();
        assert_eq!((filled.is_some(), zoom), (true, 12));
        fs::remove_dir_all(&dir).unwrap();
    }
}
