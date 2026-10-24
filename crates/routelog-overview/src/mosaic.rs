//! タイルを敷き詰めた 1 枚の写真と、それを画面の大きさに縮めて写すこと。
//!
//! ズームしていく間は、写真を少しずつ違う倍率で縮める。そのまま間引くと、細かい模様がちらつく。
//! 半分ずつ縮めた写真を前もって作っておき、倍率に近い 2 枚から取って混ぜる。

use rayon::prelude::*;
use tiny_skia::Pixmap;

use crate::view::{Flight, MIN_TILE_ZOOM, TILE_PX, TileRange, View};

/// タイル 1 枚の辺（px）。
pub const TILE: usize = TILE_PX as usize;
/// タイル 1 枚の RGB のバイト数。
pub const TILE_BYTES: usize = TILE * TILE * 3;

/// 写真の無い所の色。
const BLANK: [u8; 3] = [0x2b, 0x35, 0x40];

struct Level {
    width: usize,
    height: usize,
    rgb: Vec<u8>,
}

impl Level {
    /// 縦横を半分にする。4 px の平均を 1 px にする。
    fn half(&self) -> Level {
        let (width, height) = (self.width.div_ceil(2), self.height.div_ceil(2));
        let mut rgb = vec![0; width * height * 3];
        rgb.par_chunks_mut(width * 3)
            .enumerate()
            .for_each(|(y, row)| {
                let ys = [2 * y, (2 * y + 1).min(self.height - 1)];
                for x in 0..width {
                    let xs = [2 * x, (2 * x + 1).min(self.width - 1)];
                    for c in 0..3 {
                        let sum: u32 = ys
                            .iter()
                            .flat_map(|&sy| xs.iter().map(move |&sx| (sx, sy)))
                            .map(|(sx, sy)| u32::from(self.rgb[(sy * self.width + sx) * 3 + c]))
                            .sum();
                        row[x * 3 + c] = ((sum + 2) / 4) as u8;
                    }
                }
            });
        Level { width, height, rgb }
    }
}

/// 混ぜ方の重みの全体。重みは、これを 1 とする整数で持つ。
const ONE: u32 = 256;

/// 写真の 1 本の軸の上で、画面の 1 列（1 行）が取る 2 px と、その混ぜ方。
#[derive(Clone, Copy)]
struct Tap {
    near: usize,
    far: usize,
    /// `far` の重み。0 から `ONE` まで
    t: u32,
}

/// 画面の `count` px の並びが、写真の上でどこに当たるか。`start` は画面の 0 px の左端の位置。
fn taps(start: f64, step: f64, count: u32, scale: f64, limit: usize) -> Vec<Tap> {
    (0..count)
        .map(|i| {
            // 画面の px のまん中を、写真の px のまん中に合わせて補間する。
            let at = ((start + (f64::from(i) + 0.5) * step) * scale - 0.5)
                .clamp(0.0, (limit - 1) as f64);
            let near = at.floor() as usize;
            Tap {
                near,
                far: (near + 1).min(limit - 1),
                t: ((at - near as f64) * f64::from(ONE)).round() as u32,
            }
        })
        .collect()
}

pub struct Mosaic {
    range: TileRange,
    levels: Vec<Level>,
}

impl Mosaic {
    /// `range` のタイルを敷く台。まだ写真の無い所は、無い所の色にしておく。
    pub fn new(range: TileRange) -> Mosaic {
        let (width, height) = (
            range.columns() as usize * TILE,
            range.rows() as usize * TILE,
        );
        let rgb = BLANK.repeat(width * height);
        Mosaic {
            range,
            levels: vec![Level { width, height, rgb }],
        }
    }

    /// タイル `(x, y)` の写真を敷く。`rgb` は 256 px 四方の RGB。
    pub fn put(&mut self, x: u32, y: u32, rgb: &[u8]) {
        let level = &mut self.levels[0];
        let left = (x - self.range.x[0]) as usize * TILE;
        let top = (y - self.range.y[0]) as usize * TILE;
        for row in 0..TILE {
            let at = ((top + row) * level.width + left) * 3;
            level.rgb[at..at + TILE * 3]
                .copy_from_slice(&rgb[row * TILE * 3..(row + 1) * TILE * 3]);
        }
    }

    /// 写真を、元の `2^shrink` 分の 1 まで縮めて写せるようにする。敷き終えてから呼ぶ。
    pub fn prepare(&mut self, shrink: f64) {
        let needed = shrink.max(0.0).ceil() as usize + 1;
        while self.levels.len() < needed {
            let last = self.levels.last().expect("台には元の写真がある");
            if last.width == 1 && last.height == 1 {
                break;
            }
            self.levels.push(last.half());
        }
    }

    /// `view` の見え方で、`pixmap` の全体に写真を写す。
    pub fn draw(&self, view: &View, pixmap: &mut Pixmap) {
        let size = [pixmap.width(), pixmap.height()];
        let lod = f64::from(self.range.zoom) - view.zoom;
        paint(&self.layers(view, size, lod, 1.0), pixmap);
    }

    /// 元の写真の `2^lod` 分の 1 に縮めて写すための、縮めた写真 2 枚とその混ぜ方。
    fn layers(&self, view: &View, size: [u32; 2], lod: f64, weight: f32) -> Vec<Layer<'_>> {
        let lod = lod.clamp(0.0, (self.levels.len() - 1) as f64);
        let near = lod.floor() as usize;
        let mix = (lod - near as f64) as f32;
        let mut layers = vec![self.layer(view, size, near, weight * (1.0 - mix))];
        if mix > 0.0 {
            layers.push(self.layer(view, size, near + 1, weight * mix));
        }
        layers
    }

    /// 縮めた写真 `level` を、`view` の見え方で写すための位置。
    fn layer(
        &self,
        view: &View,
        [width, height]: [u32; 2],
        level: usize,
        weight: f32,
    ) -> Layer<'_> {
        let level = level.min(self.levels.len() - 1);
        // 画面の 1 px が、元の写真の何 px か。
        let per = (f64::from(self.range.zoom) - view.zoom).exp2();
        let world = f64::from(self.range.zoom).exp2();
        let left = view.center[0] * world
            - f64::from(self.range.x[0]) * TILE_PX
            - f64::from(width) / 2.0 * per;
        let top = view.center[1] * world
            - f64::from(self.range.y[0]) * TILE_PX
            - f64::from(height) / 2.0 * per;
        let image = &self.levels[level];
        let scale = (-(level as f64)).exp2();
        Layer {
            image,
            columns: taps(left, per, width, scale, image.width),
            rows: taps(top, per, height, scale, image.height),
            weight,
        }
    }
}

/// 画面に写す 1 枚の写真と、その重み。
struct Layer<'a> {
    image: &'a Level,
    columns: Vec<Tap>,
    rows: Vec<Tap>,
    weight: f32,
}

/// 写真を重みで混ぜて、`pixmap` の全体に写す。
fn paint(layers: &[Layer], pixmap: &mut Pixmap) {
    let width = pixmap.width() as usize;
    // 重みを、足して `ONE` になる整数にする。端数は最後の 1 枚に寄せる。
    let mut weights: Vec<u32> = layers
        .iter()
        .map(|layer| (layer.weight * ONE as f32).round() as u32)
        .collect();
    if let Some((last, others)) = weights.split_last_mut() {
        *last = ONE.saturating_sub(others.iter().sum());
    }
    pixmap
        .data_mut()
        .par_chunks_mut(width * 4)
        .enumerate()
        .for_each(|(y, out)| {
            // 1 px の 1 色が、ONE の 3 乗倍の大きさで入る。
            let mut sum = vec![0u32; width * 3];
            for (layer, &weight) in layers.iter().zip(&weights) {
                let row = layer.rows[y];
                let stride = layer.image.width * 3;
                let top = &layer.image.rgb[row.near * stride..][..stride];
                let bottom = &layer.image.rgb[row.far * stride..][..stride];
                for (x, column) in layer.columns.iter().enumerate() {
                    let (near, far) = (column.near * 3, column.far * 3);
                    for c in 0..3 {
                        let mix = |line: &[u8]| {
                            u32::from(line[near + c]) * (ONE - column.t)
                                + u32::from(line[far + c]) * column.t
                        };
                        let value = mix(top) * (ONE - row.t) + mix(bottom) * row.t;
                        sum[x * 3 + c] += (value / ONE) * weight;
                    }
                }
            }
            let half = ONE * ONE / 2;
            for (px, color) in out
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(sum.as_chunks::<3>().0)
            {
                for c in 0..3 {
                    px[c] = ((color[c] + half) / (ONE * ONE)).min(255) as u8;
                }
                px[3] = 255;
            }
        });
}

/// 寄った所は細かい写真を縮めて使い、引いた所はズームの低いタイルを使う。
///
/// 細かい写真だけで引いた所まで写すと、敷くタイルが引いた広さの 2 乗で増える。最初の大きさを
/// 25% にすると、止まった所の 16 倍の広さになる。そこで、引いていくにつれてズームの低いタイルに
/// 移す。止まった所の絵は、いつも細かい写真だけで描く。ズーム 14 と 13 のタイルは、写真と
/// 衛星画像で見た目が違うので、止まった所に混ぜない。
pub struct Pyramid {
    /// 止まった所のタイルのズームの写真。半分ずつ縮めた写真も持つ
    fine: Mosaic,
    /// ズームを 1 つずつ下げたタイルの写真。`wide[i]` は、ズームを `i + 1` 下げたもの
    wide: Vec<Mosaic>,
}

/// 細かい写真だけで描く縮め方の上限。2 の何乗分の 1 か
const FINE_UNTIL: f64 = 1.0;
/// 細かい写真から、ズームの低いタイルへ移る幅。この間は 2 つを混ぜる
const BLEND: f64 = 0.5;

impl Pyramid {
    /// 要るタイルの範囲。先頭が細かい写真で、後はズームを 1 つずつ下げたもの。
    ///
    /// `flight` は最初の見え方から止まった所までの道のり、`zoom` は止まった所のタイルのズーム。
    pub fn ranges(flight: &Flight, size: [u32; 2], zoom: u8) -> Vec<TileRange> {
        let most = (f64::from(zoom) - flight.from.zoom).max(0.0);
        // 縮め方が `near` から `far` の間の見え方を、すべて写せる範囲。道のりの途中の見え方は、
        // その両端の見え方を含む矩形に収まる。
        let cover = |near: f64, far: f64, tile_zoom: u8| {
            let ends = match (
                flight.progress_at(f64::from(zoom) - far),
                flight.progress_at(f64::from(zoom) - near),
            ) {
                (Some(start), Some(end)) => [start, end],
                _ => [0.0, 1.0],
            };
            let [a, b] =
                ends.map(|progress| TileRange::covering(&flight.at(progress), size, tile_zoom));
            a.union(&b)
        };
        let has_wide = most > FINE_UNTIL && zoom > MIN_TILE_ZOOM;
        let fine = if has_wide {
            most.min(FINE_UNTIL + BLEND)
        } else {
            most
        };
        let mut ranges = vec![cover(0.0, fine, zoom)];
        if has_wide {
            // ズームを `step` 下げたタイルは、縮め方が `step - 1` から `step + 1` の間で使う。
            for step in 1..=zoom - MIN_TILE_ZOOM {
                if f64::from(step - 1) >= most {
                    break;
                }
                let lowest = zoom - step == MIN_TILE_ZOOM;
                let lod = if lowest {
                    most
                } else {
                    most.min(f64::from(step + 1))
                };
                ranges.push(cover(f64::from(step - 1), lod, zoom - step));
            }
        }
        ranges
    }

    /// `ranges` の順に敷いた写真から作る。`widest` までの縮め方に備える。
    pub fn new(mosaics: Vec<Mosaic>, widest: &View) -> Pyramid {
        let mut mosaics = mosaics.into_iter();
        let mut fine = mosaics.next().expect("細かい写真はいつもある");
        let mut wide: Vec<Mosaic> = mosaics.collect();
        let most = f64::from(fine.range.zoom) - widest.zoom;
        if let Some(last) = wide.last_mut() {
            fine.prepare(FINE_UNTIL + BLEND);
            // いちばん低いズームより引いた所は、それを縮めて使う。
            let steps = f64::from(fine.range.zoom - last.range.zoom);
            last.prepare(most + 1.0 - steps);
        } else {
            fine.prepare(most);
        }
        Pyramid { fine, wide }
    }

    /// `view` の見え方で、`pixmap` の全体に写真を写す。
    pub fn draw(&self, view: &View, pixmap: &mut Pixmap) {
        let size = [pixmap.width(), pixmap.height()];
        let lod = (f64::from(self.fine.range.zoom) - view.zoom).max(0.0);
        let to_wide = if self.wide.is_empty() {
            0.0
        } else {
            ((lod - FINE_UNTIL) / BLEND).clamp(0.0, 1.0) as f32
        };
        let mut layers = Vec::new();
        if to_wide < 1.0 {
            layers.extend(self.fine.layers(view, size, lod, 1.0 - to_wide));
        }
        if to_wide > 0.0 {
            let near = lod.floor() as usize;
            let mix = (lod - near as f64) as f32;
            for (step, weight) in [(near, 1.0 - mix), (near + 1, mix)] {
                if weight > 0.0 {
                    let (mosaic, level) = self.step(step);
                    layers.push(mosaic.layer(view, size, level, weight * to_wide));
                }
            }
        }
        paint(&layers, pixmap);
    }

    /// ズームを `step` 下げた写真。タイルが無ければ、いちばん低いズームの写真を縮めたもの。
    fn step(&self, step: usize) -> (&Mosaic, usize) {
        let index = step.clamp(1, self.wide.len()) - 1;
        (&self.wide[index], step - 1 - index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range() -> TileRange {
        TileRange {
            zoom: 10,
            x: [4, 5],
            y: [7, 7],
        }
    }

    #[test]
    fn a_tile_is_drawn_where_it_belongs() {
        let mut mosaic = Mosaic::new(range());
        mosaic.put(5, 7, &[200; TILE_BYTES]);
        // 2 枚の境目を、そのままの大きさで見る。左は無い所の色、右は敷いた写真になる。
        let view = View {
            center: [5.0 * 256.0 / 1024.0, 7.5 * 256.0 / 1024.0],
            zoom: 10.0,
        };
        let mut pixmap = Pixmap::new(8, 2).unwrap();
        mosaic.draw(&view, &mut pixmap);
        let px = |x: usize| &pixmap.data()[x * 4..x * 4 + 4];
        assert_eq!(px(0), &[BLANK[0], BLANK[1], BLANK[2], 255]);
        assert_eq!(px(7), &[200, 200, 200, 255]);
    }

    #[test]
    fn a_shrunk_view_averages_fine_patterns() {
        // 1 px おきの縞を 4 分の 1 に縮めて見ると、平均の灰色になる。
        let mut mosaic = Mosaic::new(range());
        let stripes: Vec<u8> = (0..TILE * TILE)
            .flat_map(|i| [if i % 2 == 0 { 0 } else { 255 }; 3])
            .collect();
        mosaic.put(4, 7, &stripes);
        mosaic.prepare(2.0);
        let view = View {
            center: [4.5 * 256.0 / 1024.0, 7.5 * 256.0 / 1024.0],
            zoom: 8.0,
        };
        let mut pixmap = Pixmap::new(16, 16).unwrap();
        mosaic.draw(&view, &mut pixmap);
        let px = &pixmap.data()[(8 * 16 + 8) * 4..(8 * 16 + 8) * 4 + 3];
        assert!(px.iter().all(|&c| (120..=136).contains(&c)), "{px:?}");
    }

    /// 止まった所がズーム 14.5 の見え方と、そこから `out` だけ引いた見え方。
    fn views(out: f64) -> (View, View) {
        let stop = View {
            center: [227.0, 101.0],
            zoom: 14.5,
        };
        (
            stop,
            View {
                zoom: 14.5 - out,
                ..stop
            },
        )
    }

    fn flight((to, from): (View, View)) -> Flight {
        Flight { from, to }
    }

    #[test]
    fn wide_views_use_tiles_of_lower_zooms() {
        // 4 分の 1 から寄ると、ズーム 15 の写真を 2 の 2.5 乗分の 1 まで縮めることになる。
        let zooms: Vec<u8> = Pyramid::ranges(&flight(views(2.0)), [64, 32], 15)
            .iter()
            .map(|r| r.zoom)
            .collect();
        assert_eq!(zooms, [15, 14, 13, 12]);
        // 引かないなら、細かい写真だけでよい。
        assert_eq!(Pyramid::ranges(&flight(views(0.0)), [64, 32], 15).len(), 1);
    }

    #[test]
    fn the_tiles_cover_the_whole_way_when_the_center_moves() {
        let (stop, _) = views(0.0);
        let world = |px: f64| px / f64::from(1u32 << 15);
        // ズーム 15 の世界で、右へ 1000 px 動く。
        let moved = View {
            center: [stop.center[0] + world(1000.0), stop.center[1]],
            ..stop
        };
        let range = |flight: Flight| Pyramid::ranges(&flight, [64, 32], 15)[0];
        let (at_stop, at_moved) = (range(Flight::still(stop)), range(Flight::still(moved)));
        let both = range(Flight {
            from: moved,
            to: stop,
        });
        assert_eq!((both.x, both.y), ([at_stop.x[0], at_moved.x[1]], at_stop.y));
        assert!(at_moved.x[0] > at_stop.x[1]);
    }

    #[test]
    fn the_stopped_view_uses_only_the_fine_photo_and_wide_views_blend_into_lower_zooms() {
        let (stop, widest) = views(2.0);
        let ranges = Pyramid::ranges(&flight((stop, widest)), [64, 32], 15);
        let mosaics = ranges
            .iter()
            .map(|range| {
                let mut mosaic = Mosaic::new(*range);
                let value = if range.zoom == 15 { 200 } else { 50 };
                for (x, y) in range.tiles() {
                    mosaic.put(x, y, &[value; TILE_BYTES]);
                }
                mosaic
            })
            .collect();
        let pyramid = Pyramid::new(mosaics, &widest);
        let at = |zoom: f64| {
            let mut pixmap = Pixmap::new(64, 32).unwrap();
            pyramid.draw(&View { zoom, ..stop }, &mut pixmap);
            pixmap.data()[(16 * 64 + 32) * 4]
        };
        // 縮め方 0.5 と 1 は細かい写真だけ、1.25 は半々、1.5 から先はズームの低いタイルだけ。
        assert_eq!(
            [at(14.5), at(14.0), at(13.75), at(13.5), at(12.5)],
            [200, 200, 125, 50, 50]
        );
    }
}
