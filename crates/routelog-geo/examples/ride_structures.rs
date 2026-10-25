//! 実データで、走行の橋・トンネルに国土交通省のデータベースの施設を対応づけて確かめる。走行の
//! ファイルと判定用データは読むだけである。足りない施設のタイルは取ってきて、タイルのフォルダに
//! 置く。
//!
//! 使い方: cargo run --release -p routelog-geo --example ride_structures -- <判定用データのフォルダ>
//!             <走行の JSON> <タイルのフォルダ>

use std::path::PathBuf;

use routelog_core::{EventKind, RideDocument};
use routelog_geo::official::{self, Store};
use routelog_geo::{Options, Tiles};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, ride, tiles_dir] = args.as_slice() else {
        eprintln!(
            "使い方: ride_structures <判定用データのフォルダ> <走行の JSON> <タイルのフォルダ>"
        );
        std::process::exit(2);
    };
    let json = std::fs::read_to_string(ride).expect("走行のファイルを読めない");
    let doc: RideDocument = serde_json::from_str(&json).expect("走行のファイルの形が違う");
    let line = doc.line();

    let store = Store::new(PathBuf::from(tiles_dir));
    let tiles = official::tiles_along(&line, 150.0);
    let fetched = store.fetch(&tiles).expect("タイルを取れない");
    println!("タイル {} 枚（取った {fetched} 枚）", tiles.len() * 2);

    let mut events = Tiles::new(PathBuf::from(dir))
        .extract(&line, &Options::default())
        .expect("判定用データを読めない");
    official::attach(&mut events, &line, &store.facilities(&tiles));
    for event in events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Bridge | EventKind::Tunnel))
    {
        let length = event.detail.end_m.map_or(0.0, |end| end - event.route_m);
        println!(
            "{:7.2} km  {}  {}  （OSM {:.0} m）",
            event.route_m / 1000.0,
            if event.hidden {
                "字幕なし"
            } else {
                "字幕あり"
            },
            event.shown_name(),
            length
        );
    }
}
