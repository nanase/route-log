//! 概略図の動画を、決めた経路で書き出してみる。
//!
//! cargo run -p routelog-overview --example render -- <short|long|half> <タイルの置き場所> <出力.mp4> [見本.png]
//!
//! half は long を 2 つの回に分けた前半で、全経路を見せてから前半へ寄る。見本は、止まった所と、
//! 名前に -start を添えた最初の見え方の 2 枚を書く。出典は、文字の代わりに灰色の帯にする。

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use routelog_overview::{
    Credits, Ffmpeg, Fps, Motion, Route, Sources, Still, TileStore, preview, render,
};
use tiny_skia::{Color, Pixmap};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [kind, tiles, out, rest @ ..] = args.as_slice() else {
        eprintln!("使い方: render <short|long|half> <タイルの置き場所> <出力.mp4> [見本.png]");
        std::process::exit(2);
    };
    // short は碓氷峠のあたりの 10 km ほど、long は長野から高崎の 100 km ほど。
    let whole: Vec<[f64; 2]> = match kind.as_str() {
        "short" => vec![
            [138.596, 36.343],
            [138.62, 36.345],
            [138.645, 36.338],
            [138.66, 36.35],
            [138.68, 36.345],
            [138.7, 36.36],
        ],
        _ => vec![
            [138.194, 36.643],
            [138.25, 36.55],
            [138.33, 36.40],
            [138.45, 36.33],
            [138.596, 36.343],
            [138.7, 36.36],
            [138.88, 36.32],
            [139.003, 36.322],
        ],
    };
    // half は、碓氷峠までの前半と、その先の後半に分ける。
    let (line, others) = if kind == "half" {
        (whole[..5].to_vec(), vec![whole[4..].to_vec()])
    } else {
        (whole, Vec::new())
    };
    let route = Route {
        line: &line,
        others: &others,
    };
    let band = |width| {
        let mut pixmap = Pixmap::new(width, 44).unwrap();
        pixmap.fill(Color::from_rgba8(0, 0, 0, 120));
        pixmap
    };
    let credits = Credits {
        photo: band(900),
        landsat: band(1800),
    };
    let store = TileStore::new(PathBuf::from(tiles));
    let sources = Sources {
        tiles: &store,
        credits: &credits,
    };
    let cancel = AtomicBool::new(false);
    let motion = Motion {
        fps: Fps::FPS_59_94,
        lead_seconds: if others.is_empty() { 0.0 } else { 3.0 },
        zoom_seconds: 5.0,
        hold_seconds: 5.0,
        start_scale: 0.25,
    };
    if let Some(png) = rest.first() {
        let start = Still::Start {
            scale: motion.start_scale,
        };
        for (still, path) in [
            (Still::Stop, png.clone()),
            (start, png.replace(".png", "-start.png")),
        ] {
            let bytes = preview(route, still, [960, 540], sources, &cancel).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    }
    let ffmpeg = Ffmpeg::find(None);
    eprintln!("書き方: {:?}", ffmpeg.codec());
    let started = Instant::now();
    let mut last = None;
    render(
        route,
        motion,
        sources,
        &ffmpeg,
        &PathBuf::from(out),
        &mut |progress| {
            let phase = std::mem::discriminant(&progress);
            if last != Some(phase) {
                eprintln!("{:6.1} 秒: {progress:?}", started.elapsed().as_secs_f64());
                last = Some(phase);
            }
        },
        &cancel,
    )
    .unwrap();
    eprintln!("{:6.1} 秒: 書き終えた", started.elapsed().as_secs_f64());
}
