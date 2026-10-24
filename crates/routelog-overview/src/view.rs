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

/// フレームレート。59.94 fps（60000/1001）のような半端な値も正しく持てるよう、分数で持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fps {
    pub num: u32,
    pub den: u32,
}

impl Fps {
    pub const FPS_59_94: Fps = Fps {
        num: 60000,
        den: 1001,
    };

    /// 分母が 0 か、1 秒に 1〜120 フレームの外なら `None`。
    pub fn new(num: u32, den: u32) -> Option<Fps> {
        (den > 0 && (den..=den.saturating_mul(120)).contains(&num)).then_some(Fps { num, den })
    }

    /// `seconds` 秒の間のフレームの数。いちばん近いフレームに丸める。
    fn frames(self, seconds: f64) -> u32 {
        (seconds * f64::from(self.num) / f64::from(self.den))
            .round()
            .max(0.0) as u32
    }
}

/// ffmpeg に渡す書き方。`60000/1001` のようにする。
impl std::fmt::Display for Fps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

/// 見せ方の時間。引いた所をしばらく見せてから寄っていき、経路の全体が見える所で止まって、
/// そのまま見せる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub fps: Fps,
    /// 寄り始める前に、最初の見え方を見せる時間
    pub lead_seconds: f64,
    pub zoom_seconds: f64,
    pub hold_seconds: f64,
    /// 最初の見え方の大きさ。止まった所に対する比で、1 より小さい
    pub start_scale: f64,
}

impl Motion {
    /// 寄り始める前に、最初の見え方を見せ続けるフレームの数。寄っていく間の最初のフレームは
    /// 数えない。
    pub fn lead_frames(&self) -> u32 {
        self.fps.frames(self.lead_seconds)
    }

    /// 寄っていく間のフレームの数。最初のフレームが最初の見え方、最後のフレームが止まった所になる。
    pub fn zoom_frames(&self) -> u32 {
        self.fps
            .frames(self.lead_seconds + self.zoom_seconds)
            .saturating_sub(self.lead_frames())
            .max(1)
    }

    /// 止まった所を見せ続けるフレームの数。寄っていく間の最後のフレームは数えない。全体の長さを
    /// 丸めてから前の 2 つを引き、丸めの差が積もらないようにする。
    pub fn hold_frames(&self) -> u32 {
        self.fps
            .frames(self.lead_seconds + self.zoom_seconds + self.hold_seconds)
            .saturating_sub(self.lead_frames() + self.zoom_frames())
    }

    /// 最初の大きさを、止まった所からのズームの差にしたもの。負になる。全経路が収まらなければ、
    /// 最初の見え方はこれより引く。
    pub fn start_offset(&self) -> f64 {
        self.start_scale.log2()
    }

    /// 寄っていく間の `frame` 番目のフレームが、最初の見え方から止まった所までのどこにあるか。
    /// 0 から 1 へ進み、始めと終わりをゆっくりにする。
    pub fn progress(&self, frame: u32) -> f64 {
        let last = self.zoom_frames() - 1;
        if last == 0 {
            return 1.0;
        }
        ease(f64::from(frame.min(last)) / f64::from(last))
    }
}

/// 始めと終わりがゆっくりな動き。0 から 1 へ進む。
fn ease(t: f64) -> f64 {
    (1.0 - (PI * t).cos()) / 2.0
}

/// 最初の見え方から、止まった所へ寄っていく道のり。
///
/// 画面の上で動かない 1 点を中心に寄る。途中の見え方の四隅は、ズームの倍率の逆数に比例して
/// 動くので、両端の見え方を含む矩形の外へ出ない。ズームが変わらなければ、まっすぐ動かす。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flight {
    pub from: View,
    pub to: View,
}

impl Flight {
    /// 動かない見え方。
    pub fn still(view: View) -> Flight {
        Flight {
            from: view,
            to: view,
        }
    }

    /// 道のりの `progress`（0 から 1）の所の見え方。ズームの段は、道のりに対して線形に動く。
    pub fn at(&self, progress: f64) -> View {
        let rise = self.to.zoom - self.from.zoom;
        let zoom = self.from.zoom + rise * progress;
        // 最初の見え方の中心が、どれだけ残るか。倍率を s として (s1/s - 1) / (s1/s0 - 1)。
        let b = rise * std::f64::consts::LN_2;
        let weight = if rise.abs() <= 1e-12 {
            1.0 - progress
        } else {
            ((1.0 - progress) * b).exp_m1() / b.exp_m1()
        };
        let center =
            [0, 1].map(|i| self.to.center[i] + (self.from.center[i] - self.to.center[i]) * weight);
        View { center, zoom }
    }

    /// ズームが `zoom` の所の道のり。道のりの外なら端に寄せる。ズームが変わらなければ None。
    pub fn progress_at(&self, zoom: f64) -> Option<f64> {
        let rise = self.to.zoom - self.from.zoom;
        (rise.abs() > 1e-12).then(|| ((zoom - self.from.zoom) / rise).clamp(0.0, 1.0))
    }
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

    /// 両方を含む範囲。ズームは同じものを渡す。
    pub fn union(&self, other: &TileRange) -> TileRange {
        debug_assert_eq!(self.zoom, other.zoom);
        TileRange {
            zoom: self.zoom,
            x: [self.x[0].min(other.x[0]), self.x[1].max(other.x[1])],
            y: [self.y[0].min(other.y[0]), self.y[1].max(other.y[1])],
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
            fps: Fps::new(60, 1).unwrap(),
            lead_seconds: 0.0,
            zoom_seconds: 3.0,
            hold_seconds: 10.0,
            start_scale: 0.5,
        };
        assert_eq!((motion.zoom_frames(), motion.hold_frames()), (180, 600));
        assert_eq!(motion.progress(0), 0.0);
        assert_eq!(motion.progress(179), 1.0);
        // 始めと終わりは、まん中より動きが小さい。
        let step = |frame| motion.progress(frame + 1) - motion.progress(frame);
        assert!(step(0) < step(89) / 10.0 && step(178) < step(89) / 10.0);
    }

    #[test]
    fn a_motion_without_zoom_is_a_still() {
        let motion = Motion {
            fps: Fps::new(60, 1).unwrap(),
            lead_seconds: 0.0,
            zoom_seconds: 0.0,
            hold_seconds: 1.0,
            start_scale: 0.7,
        };
        assert_eq!((motion.zoom_frames(), motion.progress(0)), (1, 1.0));
        // 寄る間の 1 枚も、止める時間に含める。
        assert_eq!(motion.hold_frames(), 59);
    }

    #[test]
    fn the_frames_follow_the_seconds_at_59_94_fps() {
        let motion = Motion {
            fps: Fps::FPS_59_94,
            lead_seconds: 0.0,
            zoom_seconds: 5.0,
            hold_seconds: 5.0,
            start_scale: 0.25,
        };
        // 5 秒は 299.7 枚、10 秒は 599.4 枚。
        assert_eq!((motion.zoom_frames(), motion.hold_frames()), (300, 299));
        assert_eq!(Fps::FPS_59_94.to_string(), "60000/1001");
        // 3 秒は 179.8 枚、8 秒は 479.5 枚、13 秒は 779.2 枚。丸めの差は積もらない。
        let led = Motion {
            lead_seconds: 3.0,
            ..motion
        };
        assert_eq!(
            (led.lead_frames(), led.zoom_frames(), led.hold_frames()),
            (180, 300, 299)
        );
    }

    #[test]
    fn the_flight_zooms_around_a_fixed_point_and_ends_at_the_stop() {
        let flight = Flight {
            from: View {
                center: [100.0, 50.0],
                zoom: 10.0,
            },
            to: View {
                center: [100.5, 50.25],
                zoom: 12.0,
            },
        };
        assert_eq!(flight.at(0.0), flight.from);
        let end = flight.at(1.0);
        assert!((end.center[0] - 100.5).abs() < 1e-12 && (end.zoom - 12.0).abs() < 1e-12);
        // 動かない点は、どの見え方でも画面の同じ所に出る。
        let size = [3840, 2160];
        let s = [10.0f64.exp2(), 12.0f64.exp2()];
        let fixed = [0, 1]
            .map(|i| (flight.to.center[i] * s[1] - flight.from.center[i] * s[0]) / (s[1] - s[0]));
        let first = flight.from.on_screen(fixed, size);
        for progress in [0.25, 0.5, 0.9] {
            let at = flight.at(progress).on_screen(fixed, size);
            assert!((at[0] - first[0]).abs() < 1e-6 && (at[1] - first[1]).abs() < 1e-6);
        }
        assert_eq!(flight.progress_at(11.0), Some(0.5));
        assert_eq!(flight.progress_at(20.0), Some(1.0));
    }

    #[test]
    fn a_flight_without_zoom_moves_straight() {
        let flight = Flight {
            from: View {
                center: [0.0, 0.0],
                zoom: 10.0,
            },
            to: View {
                center: [2.0, 4.0],
                zoom: 10.0,
            },
        };
        assert_eq!(flight.at(0.5).center, [1.0, 2.0]);
        assert_eq!(flight.progress_at(10.0), None);
        assert_eq!(Flight::still(flight.to).at(0.3), flight.to);
    }

    #[test]
    fn a_frame_rate_needs_a_denominator_and_a_sane_speed() {
        assert_eq!(
            Fps::new(24000, 1001),
            Some(Fps {
                num: 24000,
                den: 1001
            })
        );
        assert_eq!(Fps::new(120, 1).map(|f| f.num), Some(120));
        assert_eq!(
            [Fps::new(60, 0), Fps::new(0, 1), Fps::new(121, 1)],
            [None; 3]
        );
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
