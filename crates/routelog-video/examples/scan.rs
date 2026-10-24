//! フォルダを読み、日ごとのタイムラインを表示する。
//!
//! 使い方: cargo run -p routelog-video --example scan -- <フォルダ> [時計の補正（秒）]

use std::path::PathBuf;

use routelog_core::{Timeline, group_by_date, jst};
use routelog_video::{Ffprobe, NamePattern, scan};

fn clock(unix_ms: i64) -> String {
    jst::iso(unix_ms)[11..19].to_owned()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().ok_or("フォルダを指定してください")?);
    let clock_offset_ms = args
        .next()
        .map(|s| s.parse::<i64>())
        .transpose()?
        .unwrap_or(0)
        * 1000;

    let scan = scan(&dir, &NamePattern::dji_action4(), &Ffprobe::find(None))?;
    for skipped in &scan.skipped {
        println!("読めない: {}（{}）", skipped.path, skipped.reason);
    }
    for ride in group_by_date(scan.files) {
        println!("{}  {} 本", ride.date, ride.files.len());
        let timeline = Timeline::new(ride.files, clock_offset_ms);
        for span in timeline.spans() {
            let file = &timeline.files()[span.file];
            println!(
                "  {} - {}  {}  {:>7.1} 秒  {}",
                clock(span.start_ms),
                clock(span.end_ms),
                file.name,
                file.duration_ms as f64 / 1000.0,
                file.codec
            );
            if let Some(gap) = timeline.gaps().iter().find(|g| g.start_ms == span.end_ms) {
                println!("  （録画停止 {} 秒）", (gap.end_ms - gap.start_ms) / 1000);
            }
        }
    }
    Ok(())
}
