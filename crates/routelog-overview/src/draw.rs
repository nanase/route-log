//! 写真の上に重ねる、経路の線と、始点・終点の印と、出典。
//!
//! 大きさは 4K（高さ 2160 px）で決め、ほかの大きさでは高さの比で縮める。ズームしていく間も、
//! 画面の上の太さは変えない。

use tiny_skia::{
    Color, FillRule, FilterQuality, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint,
    Rect, Stroke, Transform,
};

/// 大きさを決めた画面の高さ（px）。
const BASE_HEIGHT: f32 = 2160.0;
/// 出典の画像を作った画面の幅（px）。フロントが、この幅の画面に合わせて文字を描く。
pub const CREDIT_BASE_WIDTH: f32 = 3840.0;

const LINE: [u8; 3] = [0xff, 0xd6, 0x0a];
const OUTLINE: [u8; 3] = [0x11, 0x11, 0x11];
const START: [u8; 3] = [0x1f, 0xae, 0x4b];
const WHITE: [u8; 3] = [0xff, 0xff, 0xff];
/// 経路の縁の太さと、線の太さ（px）。
const OUTLINE_WIDTH: f32 = 22.0;
const LINE_WIDTH: f32 = 12.0;
/// 薄く描く経路の、止まった所での不透明度。
const FAINT: f32 = 0.4;

fn paint([r, g, b]: [u8; 3]) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(r, g, b, 255));
    paint.anti_alias = true;
    paint
}

/// 画面の大きさに対する、4K からの比。
pub fn scale_of(pixmap: &Pixmap) -> f32 {
    pixmap.height() as f32 / BASE_HEIGHT
}

/// 経路の線。黒い縁の上に、黄色の線を重ねる。
pub fn route(pixmap: &mut Pixmap, points: &[[f32; 2]]) {
    stroke_route(pixmap, points, scale_of(pixmap), [0.0, 0.0]);
}

/// 寄っていく道のりの `progress`（0 から 1）の所での、薄く描く経路の不透明度。最初ははっきり
/// 描き、止まった所で `FAINT` になる。
pub fn fade(progress: f64) -> f32 {
    1.0 - (1.0 - FAINT) * progress as f32
}

/// 半透明の経路の線。`opacity` は 0 から 1。線の重なった所が濃くならないよう、別の板に描いてから
/// まとめて重ねる。板は、線の掛かる所だけの大きさにする。
pub fn faded_routes(pixmap: &mut Pixmap, lines: &[Vec<[f32; 2]>], opacity: f32) {
    if opacity >= 1.0 {
        for points in lines {
            route(pixmap, points);
        }
        return;
    }
    let s = scale_of(pixmap);
    let pad = OUTLINE_WIDTH / 2.0 * s + 2.0;
    let (mut min, mut max) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for p in lines.iter().flatten() {
        min = [min[0].min(p[0]), min[1].min(p[1])];
        max = [max[0].max(p[0]), max[1].max(p[1])];
    }
    let bound = [pixmap.width() as f32, pixmap.height() as f32];
    let [left, top] = [0, 1].map(|i| (min[i] - pad).floor().clamp(0.0, bound[i]));
    let [right, bottom] = [0, 1].map(|i| (max[i] + pad).ceil().clamp(0.0, bound[i]));
    let Some(mut layer) = Pixmap::new((right - left) as u32, (bottom - top) as u32) else {
        return;
    };
    for points in lines {
        stroke_route(&mut layer, points, s, [left, top]);
    }
    let paint = PixmapPaint {
        opacity,
        ..PixmapPaint::default()
    };
    pixmap.draw_pixmap(
        left as i32,
        top as i32,
        layer.as_ref(),
        &paint,
        Transform::identity(),
        None,
    );
}

/// 経路の線を、`s` の大きさで描く。`origin` は、`pixmap` の左上が画面のどこに当たるか。
fn stroke_route(pixmap: &mut Pixmap, points: &[[f32; 2]], s: f32, origin: [f32; 2]) {
    let mut builder = PathBuilder::new();
    let Some(first) = points.first() else {
        return;
    };
    builder.move_to(first[0], first[1]);
    for p in &points[1..] {
        builder.line_to(p[0], p[1]);
    }
    // 点が 1 つだけでも、丸い端で点として出す。
    if points.len() == 1 {
        builder.line_to(first[0], first[1]);
    }
    let Some(path) = builder.finish() else {
        return;
    };
    let transform = Transform::from_translate(-origin[0], -origin[1]);
    for (color, width) in [(OUTLINE, OUTLINE_WIDTH), (LINE, LINE_WIDTH)] {
        let stroke = Stroke {
            width: width * s,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint(color), &stroke, transform, None);
    }
}

fn circle(pixmap: &mut Pixmap, at: [f32; 2], radius: f32, color: [u8; 3]) {
    if let Some(path) = PathBuilder::from_circle(at[0], at[1], radius) {
        pixmap.fill_path(
            &path,
            &paint(color),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn rect(pixmap: &mut Pixmap, left: f32, top: f32, width: f32, height: f32, color: [u8; 3]) {
    if let Some(rect) = Rect::from_xywh(left, top, width, height) {
        pixmap.fill_rect(rect, &paint(color), Transform::identity(), None);
    }
}

/// 始点の印。白で縁取った緑の丸。
pub fn start(pixmap: &mut Pixmap, at: [f32; 2]) {
    let s = scale_of(pixmap);
    circle(pixmap, at, 28.0 * s, OUTLINE);
    circle(pixmap, at, 25.0 * s, WHITE);
    circle(pixmap, at, 17.0 * s, START);
}

/// 終点の印。終点に立てた竿に、白黒の市松の旗を付ける。
pub fn finish(pixmap: &mut Pixmap, at: [f32; 2]) {
    let s = scale_of(pixmap);
    let [x, y] = at;
    let (pole, pole_width) = (130.0 * s, 8.0 * s);
    let (square, columns, rows) = (16.0 * s, 6, 4);
    let (flag_width, flag_height) = (square * columns as f32, square * rows as f32);
    let edge = 3.0 * s;
    // 竿。縁を付けて、暗い写真の上でも見えるようにする。
    rect(
        pixmap,
        x - pole_width / 2.0 - edge,
        y - pole - edge,
        pole_width + 2.0 * edge,
        pole + 2.0 * edge,
        WHITE,
    );
    rect(
        pixmap,
        x - pole_width / 2.0,
        y - pole,
        pole_width,
        pole,
        OUTLINE,
    );
    // 旗。竿の上の端から右へ出す。
    let (left, top) = (x + pole_width / 2.0, y - pole);
    rect(
        pixmap,
        left - edge,
        top - edge,
        flag_width + 2.0 * edge,
        flag_height + 2.0 * edge,
        OUTLINE,
    );
    for row in 0..rows {
        for column in 0..columns {
            let color = if (row + column) % 2 == 0 {
                WHITE
            } else {
                OUTLINE
            };
            let (cx, cy) = (left + column as f32 * square, top + row as f32 * square);
            rect(pixmap, cx, cy, square, square, color);
        }
    }
    // 竿の根もと。経路の終わりの位置を示す。
    circle(pixmap, at, 14.0 * s, OUTLINE);
    circle(pixmap, at, 10.0 * s, WHITE);
}

/// 出典を、右下に置く。`credit` は幅 3840 px の画面に合わせて作った画像。
pub fn credit(pixmap: &mut Pixmap, credit: &Pixmap) {
    let k = pixmap.width() as f32 / CREDIT_BASE_WIDTH;
    let margin = 28.0 * scale_of(pixmap);
    let left = pixmap.width() as f32 - credit.width() as f32 * k - margin;
    let top = pixmap.height() as f32 - credit.height() as f32 * k - margin;
    let paint = PixmapPaint {
        quality: FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    let transform = Transform::from_scale(k, k).post_translate(left, top);
    pixmap.draw_pixmap(0, 0, credit.as_ref(), &paint, transform, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * pixmap.width() + x) * 4) as usize;
        pixmap.data()[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn the_route_is_yellow_with_a_dark_edge() {
        let mut pixmap = Pixmap::new(400, 2160).unwrap();
        route(&mut pixmap, &[[20.0, 100.5], [380.0, 100.5]]);
        // 黄色は 12 px、縁を含めて 22 px の太さになる。
        assert_eq!(px(&pixmap, 200, 100)[..3], [0xff, 0xd6, 0x0a]);
        assert_eq!(px(&pixmap, 200, 92)[..3], [0x11, 0x11, 0x11]);
        assert_eq!(px(&pixmap, 200, 85), [0, 0, 0, 0]);
    }

    #[test]
    fn other_routes_fade_while_zooming_in() {
        for (progress, opacity) in [(0.0, 1.0), (0.5, 0.7), (1.0, FAINT)] {
            assert!((fade(progress) - opacity).abs() < 1e-6);
        }
    }

    #[test]
    fn faded_routes_are_see_through_and_do_not_darken_where_they_cross() {
        let mut pixmap = Pixmap::new(400, 2160).unwrap();
        pixmap.fill(Color::from_rgba8(0, 0, 255, 255));
        let lines = [
            vec![[20.0, 100.5], [380.0, 100.5]],
            vec![[200.5, 20.0], [200.5, 380.0]],
        ];
        faded_routes(&mut pixmap, &lines, 0.4);
        // 黄色の 4 割と、下の青の 6 割。交わる所も同じ色になる。
        let yellow = [0xff, 0xd6, 0x0a].map(|c: u8| (f32::from(c) * 0.4).round() as u8);
        for at in [px(&pixmap, 100, 100), px(&pixmap, 200, 100)] {
            assert!(at[0].abs_diff(yellow[0]) <= 1 && at[1].abs_diff(yellow[1]) <= 1);
            assert!(at[2].abs_diff(yellow[2] + 153) <= 1, "{at:?}");
        }
        // 線の外は、そのまま。
        assert_eq!(px(&pixmap, 100, 300), [0, 0, 255, 255]);
    }

    #[test]
    fn the_marks_are_drawn_around_the_points() {
        let mut pixmap = Pixmap::new(3840, 2160).unwrap();
        start(&mut pixmap, [500.0, 1000.0]);
        finish(&mut pixmap, [2000.0, 1000.0]);
        assert_eq!(px(&pixmap, 500, 1000), [0x1f, 0xae, 0x4b, 0xff]);
        // 旗の左上の升は白、その右は黒。
        let (left, top) = (2004, 870);
        assert_eq!(px(&pixmap, left + 8, top + 8), [0xff; 4]);
        assert_eq!(px(&pixmap, left + 24, top + 8), [0x11, 0x11, 0x11, 0xff]);
    }

    #[test]
    fn the_credit_goes_to_the_bottom_right_scaled_to_the_screen() {
        let mut image = Pixmap::new(400, 40).unwrap();
        image.fill(Color::from_rgba8(255, 0, 0, 255));
        let mut pixmap = Pixmap::new(960, 540).unwrap();
        credit(&mut pixmap, &image);
        // 4 分の 1 の 100 × 10 px が、右下から 7 px 空けて置かれる。
        assert_eq!(px(&pixmap, 960 - 7 - 50, 540 - 7 - 5)[0], 255);
        assert_eq!(px(&pixmap, 960 - 7 - 110, 540 - 7 - 5), [0, 0, 0, 0]);
    }
}
