//! 地図の投影と、見せる範囲、ズームの動き。
//!
//! 座標は、ウェブメルカトルのズーム 0 の世界座標（px）で持つ。世界の全体が 256 px 四方になる。

use std::f64::consts::PI;

use crate::LonLat;

/// タイル 1 枚の辺（px）。
pub const TILE_PX: f64 = 256.0;
/// 経路のまわりに空ける幅。フレームの幅と高さに対する、片側の割合。旗が上にはみ出しても切れない
const MARGIN: f64 = 0.1;
/// 短い経路でも、これより寄らない。写真のタイルは 18 までで、これ以上寄るとぼける
const MAX_ZOOM: f64 = 17.0;
/// 写真のタイルのズームの範囲。8 以下は別の衛星画像になり、出典の書き方が変わるので使わない
pub const MIN_TILE_ZOOM: u8 = 9;
pub const MAX_TILE_ZOOM: u8 = 18;

/// 経度・緯度を、ズーム 0 の世界座標にする。
pub fn project([lon, lat]: LonLat) -> [f64; 2] {
    let sin = lat.to_radians().sin().clamp(-0.9999, 0.9999);
    let x = (lon + 180.0) / 360.0 * TILE_PX;
    let y = (0.5 - ((1.0 + sin) / (1.0 - sin)).ln() / (4.0 * PI)) * TILE_PX;
    [x, y]
}

/// 地図の見え方。まん中の世界座標と、ズーム。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub center: [f64; 2],
    pub zoom: f64,
}

impl View {
    /// 世界座標を、`size` の画面の座標にする。
    pub fn on_screen(self, world: [f64; 2], size: [u32; 2]) -> [f64; 2] {
        let scale = self.zoom.exp2();
        [
            (world[0] - self.center[0]) * scale + f64::from(size[0]) / 2.0,
            (world[1] - self.center[1]) * scale + f64::from(size[1]) / 2.0,
        ]
    }

    /// 経路の全体が、まわりを空けて `size` の画面に収まる見え方。点が無ければ None。
    pub fn fit(points: &[[f64; 2]], size: [u32; 2]) -> Option<View> {
        let first = points.first()?;
        let (mut min, mut max) = (*first, *first);
        for p in points {
            min = [min[0].min(p[0]), min[1].min(p[1])];
            max = [max[0].max(p[0]), max[1].max(p[1])];
        }
        let room = size.map(|n| f64::from(n) * (1.0 - 2.0 * MARGIN));
        let zoom = [0, 1]
            .map(|i| (room[i] / (max[i] - min[i])).log2())
            .into_iter()
            .fold(MAX_ZOOM, f64::min);
        Some(View {
            center: [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0],
            zoom,
        })
    }

    /// この見え方を `size` の画面に出すのに使う、写真のタイルのズーム。画面の 1 px に、タイルの
    /// 1 px 以上が当たるようにする。
    pub fn tile_zoom(&self) -> u8 {
        (self.zoom - 1e-9)
            .ceil()
            .clamp(f64::from(MIN_TILE_ZOOM), f64::from(MAX_TILE_ZOOM)) as u8
    }
}

/// ズームの動き。少し引いた所から寄っていき、経路の全体が見える所で止まって、そのまま見せる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub fps: u32,
    pub zoom_seconds: f64,
    pub hold_seconds: f64,
    /// 最初の見え方の大きさ。止まった所に対する比で、1 より小さい
    pub start_scale: f64,
}

impl Motion {
    /// 寄っていく間のフレームの数。最後のフレームが、止まった所になる。
    pub fn zoom_frames(&self) -> u32 {
        ((self.zoom_seconds * f64::from(self.fps)).round() as u32).max(1)
    }

    /// 止まった所を見せ続けるフレームの数。寄っていく間の最後のフレームは数えない。
    pub fn hold_frames(&self) -> u32 {
        (self.hold_seconds * f64::from(self.fps)).round().max(0.0) as u32
    }

    /// 最初のフレームの、止まった所からのズームの差。負になる。
    pub fn start_offset(&self) -> f64 {
        self.start_scale.log2()
    }

    /// `frame` 番目のフレームの、止まった所からのズームの差。始めと終わりをゆっくりにする。
    pub fn offset(&self, frame: u32) -> f64 {
        let last = self.zoom_frames() - 1;
        if last == 0 {
            return 0.0;
        }
        let t = f64::from(frame.min(last)) / f64::from(last);
        self.start_offset() * (1.0 - ease(t))
    }
}

/// 始めと終わりがゆっくりな動き。0 から 1 へ進む。
fn ease(t: f64) -> f64 {
    (1.0 - (PI * t).cos()) / 2.0
}

/// タイルの番号の範囲。両端を含む。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileRange {
    pub zoom: u8,
    pub x: [u32; 2],
    pub y: [u32; 2],
}

impl TileRange {
    /// `view` を `size` の画面に出すのに要るタイル。`zoom` は、タイルのズーム。
    pub fn covering(view: &View, size: [u32; 2], zoom: u8) -> TileRange {
        // 画面の 1 px が、タイルのズームの世界で何 px か。縁で補間するぶん、2 px 広く取る。
        let per = (f64::from(zoom) - view.zoom).exp2();
        let count = 1u32 << zoom;
        let range = |center: f64, side: u32| {
            let half = f64::from(side) / 2.0 * per + 2.0;
            let center = center * f64::from(zoom).exp2();
            let tile = |px: f64| ((px / TILE_PX).floor().max(0.0) as u32).min(count - 1);
            [tile(center - half), tile(center + half)]
        };
        TileRange {
            zoom,
            x: range(view.center[0], size[0]),
            y: range(view.center[1], size[1]),
        }
    }

    pub fn columns(&self) -> u32 {
        self.x[1] - self.x[0] + 1
    }

    pub fn rows(&self) -> u32 {
        self.y[1] - self.y[0] + 1
    }

    pub fn len(&self) -> usize {
        self.columns() as usize * self.rows() as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn tiles(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        (self.y[0]..=self.y[1]).flat_map(move |y| (self.x[0]..=self.x[1]).map(move |x| (x, y)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_projection_puts_the_origin_at_the_center() {
        assert_eq!(project([0.0, 0.0]), [128.0, 128.0]);
        let [x, y] = project([180.0, 85.0]);
        assert!((x - 256.0).abs() < 1e-9 && y < 1.0);
    }

    #[test]
    fn the_route_fits_inside_the_margins() {
        let points = [[100.0, 50.0], [100.5, 50.25]];
        let view = View::fit(&points, [3840, 2160]).unwrap();
        // 横は 0.5 px を 3072 px に、縦は 0.25 px を 1728 px に。狭いほうに合わせる。
        assert!((view.zoom - (3072.0f64 / 0.5).log2()).abs() < 1e-9);
        assert_eq!(view.center, [100.25, 50.125]);
        let corner = view.on_screen([100.0, 50.0], [3840, 2160]);
        assert!((corner[0] - 384.0).abs() < 1e-6);
    }

    #[test]
    fn a_single_point_is_shown_at_the_closest_zoom() {
        let view = View::fit(&[[1.0, 1.0]], [960, 540]).unwrap();
        assert_eq!(view.zoom, MAX_ZOOM);
        assert_eq!(view.tile_zoom(), 17);
        assert!(View::fit(&[], [960, 540]).is_none());
    }

    #[test]
    fn the_tile_zoom_is_never_coarser_than_the_screen() {
        let at = |zoom: f64| {
            View {
                center: [0.0, 0.0],
                zoom,
            }
            .tile_zoom()
        };
        assert_eq!(
            [at(12.0), at(12.1), at(12.9), at(3.0), at(19.0)],
            [12, 13, 13, 9, 18]
        );
    }

    #[test]
    fn the_zoom_eases_in_and_out_and_ends_where_it_stops() {
        let motion = Motion {
            fps: 60,
            zoom_seconds: 3.0,
            hold_seconds: 10.0,
            start_scale: 0.5,
        };
        assert_eq!((motion.zoom_frames(), motion.hold_frames()), (180, 600));
        assert_eq!(motion.offset(0), -1.0);
        assert_eq!(motion.offset(179), 0.0);
        // 始めと終わりは、まん中より動きが小さい。
        let step = |frame| motion.offset(frame + 1) - motion.offset(frame);
        assert!(step(0) < step(89) / 10.0 && step(178) < step(89) / 10.0);
    }

    #[test]
    fn a_motion_without_zoom_is_a_still() {
        let motion = Motion {
            fps: 60,
            zoom_seconds: 0.0,
            hold_seconds: 1.0,
            start_scale: 0.7,
        };
        assert_eq!((motion.zoom_frames(), motion.offset(0)), (1, 0.0));
    }

    #[test]
    fn the_covering_tiles_contain_the_screen() {
        // ズーム 10 の世界で、まん中が (512, 300) px にある見え方を、512 × 256 の画面に出す。
        let view = View {
            center: [0.5, 300.0 / 1024.0],
            zoom: 10.0,
        };
        let range = TileRange::covering(&view, [512, 256], 10);
        assert_eq!((range.x, range.y), ([0, 3], [0, 1]));
        assert_eq!(range.len(), 8);
    }
}
