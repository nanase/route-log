//! 概略図の動画を、決めた経路で書き出してみる。
//!
//! cargo run -p routelog-overview --example render -- <short|long> <タイルの置き場所> <出力.mp4> [見本.png]
//!
//! 出典は、文字の代わりに灰色の帯にする。

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use routelog_overview::{Credits, Ffmpeg, Motion, Sources, TileStore, preview, render};
use tiny_skia::{Color, Pixmap};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [kind, tiles, out, rest @ ..] = args.as_slice() else {
        eprintln!("使い方: render <short|long> <タイルの置き場所> <出力.mp4> [見本.png]");
        std::process::exit(2);
    };
    // short は碓氷峠のあたりの 10 km ほど、long は長野から高崎の 100 km ほど。
    let line: Vec<[f64; 2]> = match kind.as_str() {
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
    if let Some(png) = rest.first() {
        let bytes = preview(&line, [960, 540], sources, &cancel).unwrap();
        std::fs::write(png, bytes).unwrap();
    }
    let motion = Motion {
        fps: 60,
        zoom_seconds: 5.0,
        hold_seconds: 5.0,
        start_scale: 0.25,
    };
    let ffmpeg = Ffmpeg::find(None);
    eprintln!("書き方: {:?}", ffmpeg.codec());
    let started = Instant::now();
    let mut last = None;
    render(
        &line,
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
