//! イベントの通過時刻の一覧を書き出す。

use crate::event::{Event, EventKind, EventStatus};
use crate::jst;
use crate::prefecture::route_name;
use crate::timeline::Timeline;

const HEADER: &str = "time_jst,source_file,source_offset,kind,label,routes,status";

fn kind_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Departure => "departure",
        EventKind::Arrival => "arrival",
        EventKind::PrefBoundary => "pref_boundary",
        EventKind::MuniBoundary => "muni_boundary",
        EventKind::Road => "road",
        EventKind::Bridge => "bridge",
        EventKind::Tunnel => "tunnel",
        EventKind::MichiNoEki => "michi_no_eki",
        EventKind::Waypoint => "waypoint",
        EventKind::Custom => "custom",
    }
}

/// 値にカンマ・引用符・改行があれば、引用符で囲む。
fn field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// `hh:mm:ss`。端数は切り捨てる。
fn offset(ms: i64) -> String {
    let seconds = ms / 1000;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// 字幕にするイベントを、1 行 1 件の CSV にする。位置の順に並ぶ。
///
/// 動画の中の位置（ファイル名とファイル内の時:分:秒）も添える。時刻が推定できていない
/// イベントは時刻を空にし、録画していない時刻のイベントは動画の位置を空にする。
pub fn csv(events: &[Event], timeline: &Timeline) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    for event in events.iter().filter(|e| !e.hidden) {
        let position = event.time_ms.and_then(|t| timeline.locate(t));
        let routes: Vec<String> = event.detail.routes.iter().map(route_name).collect();
        let row = [
            event.time_ms.map(jst::iso).unwrap_or_default(),
            position
                .map(|p| timeline.files()[p.file].name.clone())
                .unwrap_or_default(),
            position.map(|p| offset(p.offset_ms)).unwrap_or_default(),
            kind_name(event.kind).to_owned(),
            event.label.clone(),
            routes.join(";"),
            match event.status {
                EventStatus::Confirmed => "confirmed",
                EventStatus::Estimated => "estimated",
            }
            .to_owned(),
        ];
        let fields: Vec<String> = row.iter().map(|value| field(value)).collect();
        out.push_str(&fields.join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventDetail, RouteClass, RouteRef};
    use crate::timeline::VideoFile;

    /// 2026-10-03 05:05:00（JST）に始まる 30 分のファイルと、05:54:12 に始まる 30 分のファイル。
    fn timeline() -> Timeline {
        let file = |name: &str, hour, minute, second| VideoFile {
            path: format!("videos/{name}"),
            name: name.into(),
            seq: 0,
            start_ms: jst::to_unix_ms(&jst::Civil {
                year: 2026,
                month: 10,
                day: 3,
                hour,
                minute,
                second,
                millisecond: 0,
            })
            .unwrap(),
            duration_ms: 1_800_000,
            codec: "hevc".into(),
        };
        Timeline::new(
            vec![
                file("DJI_20261003050500_0004_D.MP4", 5, 5, 0),
                file("DJI_20261003055412_0005_D.MP4", 5, 54, 12),
            ],
            0,
        )
    }

    fn at(hour: i64, minute: i64, second: i64) -> Option<i64> {
        jst::to_unix_ms(&jst::Civil {
            year: 2026,
            month: 10,
            day: 3,
            hour,
            minute,
            second,
            millisecond: 0,
        })
    }

    fn event(kind: EventKind, label: &str, time_ms: Option<i64>, status: EventStatus) -> Event {
        Event {
            id: String::new(),
            kind,
            label: label.into(),
            route_m: 0.0,
            time_ms,
            status,
            hidden: false,
            detail: EventDetail::default(),
        }
    }

    #[test]
    fn each_event_is_a_row_with_its_place_in_the_video() {
        let mut road = event(
            EventKind::Road,
            "国道19号・国道403号",
            at(5, 31, 2),
            EventStatus::Estimated,
        );
        road.detail.routes = vec![
            RouteRef {
                class: RouteClass::National,
                pref: String::new(),
                number: 19,
            },
            RouteRef {
                class: RouteClass::National,
                pref: String::new(),
                number: 403,
            },
        ];
        let events = [
            event(
                EventKind::Departure,
                "長野市役所前 出発",
                at(5, 10, 20),
                EventStatus::Confirmed,
            ),
            road,
            event(
                EventKind::MichiNoEki,
                "道の駅 しなの",
                at(6, 2, 45),
                EventStatus::Estimated,
            ),
        ];
        assert_eq!(
            csv(&events, &timeline()),
            "time_jst,source_file,source_offset,kind,label,routes,status\n\
             2026-10-03T05:10:20+09:00,DJI_20261003050500_0004_D.MP4,00:05:20,departure,長野市役所前 出発,,confirmed\n\
             2026-10-03T05:31:02+09:00,DJI_20261003050500_0004_D.MP4,00:26:02,road,国道19号・国道403号,国道19号;国道403号,estimated\n\
             2026-10-03T06:02:45+09:00,DJI_20261003055412_0005_D.MP4,00:08:33,michi_no_eki,道の駅 しなの,,estimated\n"
        );
    }

    #[test]
    fn hidden_events_are_left_out() {
        // 時刻を確定してあっても、字幕にしないものは書き出さない。
        let mut short = event(
            EventKind::Bridge,
            "短い橋",
            at(5, 10, 0),
            EventStatus::Confirmed,
        );
        short.hidden = true;
        let events = [
            short,
            event(
                EventKind::Tunnel,
                "白地トンネル",
                at(5, 11, 0),
                EventStatus::Estimated,
            ),
        ];
        let text = csv(&events, &timeline());
        assert!(!text.contains("短い橋"));
        assert!(text.contains("白地トンネル"));
    }

    #[test]
    fn what_is_not_known_is_left_empty() {
        let events = [
            // 時刻を推定できていない。
            event(
                EventKind::Tunnel,
                "先のトンネル",
                None,
                EventStatus::Estimated,
            ),
            // 録画を止めていた時刻。
            event(
                EventKind::Custom,
                "休憩",
                at(5, 45, 0),
                EventStatus::Confirmed,
            ),
        ];
        let text = csv(&events, &timeline());
        let rows: Vec<&str> = text.lines().skip(1).collect();
        assert_eq!(
            rows,
            [
                ",,,tunnel,先のトンネル,,estimated",
                "2026-10-03T05:45:00+09:00,,,custom,休憩,,confirmed"
            ]
        );
    }

    #[test]
    fn a_label_with_a_comma_or_a_quote_is_quoted() {
        let events = [event(
            EventKind::Custom,
            "左へ, \"旧道\" に入る",
            None,
            EventStatus::Estimated,
        )];
        assert!(
            csv(&events, &timeline()).contains(",custom,\"左へ, \"\"旧道\"\" に入る\",,estimated")
        );
    }

    #[test]
    fn without_videos_the_times_are_still_written() {
        let events = [event(
            EventKind::Departure,
            "出発",
            at(5, 10, 20),
            EventStatus::Confirmed,
        )];
        let text = csv(&events, &Timeline::new(vec![], 0));
        assert!(text.ends_with("2026-10-03T05:10:20+09:00,,,departure,出発,,confirmed\n"));
    }
}
