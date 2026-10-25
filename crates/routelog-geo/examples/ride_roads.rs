//! 実データで、走行の経路から抽出した路線のイベントを確かめる。走行のファイルも判定用データも
//! 読むだけで、何も書かない。
//!
//! 使い方: cargo run --release -p routelog-geo --example ride_roads -- <判定用データのフォルダ>
//!             <走行の JSON> <始めの km> <終わりの km>

use std::path::PathBuf;
use std::time::Instant;

use routelog_core::{EventKind, RideDocument};
use routelog_geo::{Options, Tiles};

/// 閾値。環境変数 `JUNCTION_M`・`JUNCTION_MERGE_M` があれば、交差点の閾値をそれで試す。
fn options() -> Options {
    let read = |name: &str| std::env::var(name).ok().and_then(|v| v.parse().ok());
    let mut options = Options::default();
    if let Some(m) = read("JUNCTION_M") {
        options.junction_m = m;
    }
    if let Some(m) = read("JUNCTION_MERGE_M") {
        options.junction_merge_m = m;
    }
    options
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, ride, from_km, to_km] = args.as_slice() else {
        eprintln!(
            "使い方: ride_roads <判定用データのフォルダ> <走行の JSON> <始めの km> <終わりの km>"
        );
        std::process::exit(2);
    };
    let km = |text: &str| text.parse::<f64>().expect("km は数で書く") * 1000.0;
    let (from_m, to_m) = (km(from_km), km(to_km));
    let json = std::fs::read_to_string(ride).expect("走行のファイルを読めない");
    let doc: RideDocument = serde_json::from_str(&json).expect("走行のファイルの形が違う");

    let started = Instant::now();
    let events = Tiles::new(PathBuf::from(dir))
        .extract(&doc.line(), &options())
        .expect("判定用データを読めない");
    println!("抽出 {:?}", started.elapsed());
    for event in events
        .iter()
        .filter(|e| e.kind == EventKind::Road && (from_m..=to_m).contains(&e.route_m))
    {
        println!("{:7.2} km  {}", event.route_m / 1000.0, event.label);
    }
}
