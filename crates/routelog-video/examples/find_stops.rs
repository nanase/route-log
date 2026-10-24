//! 動画 1 本の停車の候補を出す。規則を確かめるときに使う。
//!
//! 使い方: `cargo run -p routelog-video --example find_stops -- <動画> [測った値の JSON の書き先]`
//!
//! ffmpeg と ffprobe は、環境変数 `ROUTELOG_FFMPEG`・`ROUTELOG_FFPROBE` か PATH から探す。

use std::path::PathBuf;
use std::time::Instant;

use routelog_video::{Ffprobe, StopMeter, stops};

fn main() {
    let mut args = std::env::args_os().skip(1);
    let Some(path) = args.next().map(PathBuf::from) else {
        eprintln!("使い方: find_stops <動画> [測った値の JSON の書き先]");
        std::process::exit(2);
    };
    let out = args.next().map(PathBuf::from);

    let probe = Ffprobe::find(None)
        .run(&path)
        .expect("ffprobe で長さを読めない");
    let started = Instant::now();
    let mut shown = -1;
    let measured = StopMeter::find(None)
        .measure(
            &path,
            probe.duration_ms,
            &mut |done| {
                let percent = (done * 100.0) as i32;
                if percent / 10 != shown / 10 {
                    shown = percent;
                    eprintln!("{percent}%  {:.0} 秒", started.elapsed().as_secs_f64());
                }
            },
            &|| false,
        )
        .expect("測れない");
    let fine = measured.fine_pct.iter().filter(|v| v.is_some()).count();
    eprintln!(
        "{:.0} 秒で測った（細かく測った秒 {fine} / {}）",
        started.elapsed().as_secs_f64(),
        measured.audio_db.len()
    );
    if let Some(out) = out {
        std::fs::write(&out, serde_json::to_string(&measured).unwrap()).unwrap();
    }
    for candidate in stops::candidates(&measured) {
        let start = candidate.start_ms / 1000;
        println!(
            "{:02}:{:02}:{:02} {:5} 秒",
            start / 3600,
            start / 60 % 60,
            start % 60,
            (candidate.end_ms - candidate.start_ms) / 1000
        );
    }
}
