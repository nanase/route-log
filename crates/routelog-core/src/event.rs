//! 字幕にするイベント。経路の上の位置と、そこを通った時刻を持つ。

use serde::{Deserialize, Serialize};

/// 経路を引き直したとき、同じイベントとみなす位置のずれの上限（m）。
pub const CARRY_OVER_M: f64 = 200.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Departure,
    Arrival,
    PrefBoundary,
    MuniBoundary,
    Road,
    Bridge,
    Tunnel,
    MichiNoEki,
    Custom,
}

impl EventKind {
    /// 経路から機械的に抽出する種類。残りは人が足す。
    pub fn is_extracted(self) -> bool {
        !matches!(self, Self::Departure | Self::Arrival | Self::Custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    /// 時刻は推定。
    Estimated,
    /// 時刻を人が確定した。
    Confirmed,
    /// 字幕にしない。
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteClass {
    National,
    Prefectural,
}

/// 路線 1 本。標識の画像を選ぶ手がかりになる。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RouteRef {
    pub class: RouteClass,
    /// 都道府県道の所属。`nagano` のような地域名。国道では空。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pref: String,
    pub number: u32,
}

/// 種類ごとの付帯情報。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EventDetail {
    /// `road`：重用区間を含む全路線。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<RouteRef>,
    /// `bridge`・`tunnel`：終わりの位置（m）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_m: Option<f64>,
    /// 抽出したときの名前。人が名前を直しても、抽出し直したときに同じイベントだと分かる。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracted_label: Option<String>,
    /// 動画を見て確定したときの、カメラの時計での時刻（Unix ミリ秒）。
    ///
    /// 通過時刻はこれに時計の補正値を足したものになる。補正値を後から変えると、
    /// 動画で確定した時刻も一緒に動く。手で入力した時刻は持たない。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_ms: Option<i64>,
    /// 位置ではなく時刻が分かっているイベント。位置は時刻から求め、時刻推定の基準にはしない。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub by_time: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// UUIDv7。抽出したばかりで、まだ保存していないものは空。
    pub id: String,
    pub kind: EventKind,
    pub label: String,
    /// 経路の始点からの距離（m）。
    pub route_m: f64,
    /// 通過時刻（Unix ミリ秒）。推定できないときは `None`。
    pub time_ms: Option<i64>,
    pub status: EventStatus,
    #[serde(default)]
    pub detail: EventDetail,
}

/// 経路から抽出し直したイベントに、人が手を入れた結果を引き継ぐ。
///
/// - 抽出したイベントは、同じ種類で、抽出したときの名前が同じで、位置が近い元のイベントが
///   あれば、その ID・状態・時刻・人が直した名前を受け継ぐ。位置は新しい経路のものにする
/// - 人が足したイベント（出発・到着・手動）は、そのまま残す
/// - 受け継ぐ相手のいない元のイベントは消える
///
/// 結果は位置の順に並ぶ。
pub fn carry_over(existing: &[Event], extracted: Vec<Event>) -> Vec<Event> {
    let extracted_label = |event: &Event| -> String {
        let detail = &event.detail;
        detail
            .extracted_label
            .clone()
            .unwrap_or_else(|| event.label.clone())
    };
    let mut taken = vec![false; existing.len()];
    let mut events: Vec<Event> = extracted
        .into_iter()
        .map(|mut event| {
            let nearest = existing
                .iter()
                .enumerate()
                .filter(|(i, old)| {
                    !taken[*i] && old.kind == event.kind && extracted_label(old) == event.label
                })
                .map(|(i, old)| (i, (old.route_m - event.route_m).abs()))
                .filter(|(_, distance)| *distance <= CARRY_OVER_M)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((i, _)) = nearest {
                taken[i] = true;
                let old = &existing[i];
                event.detail.extracted_label = Some(event.label.clone());
                event.detail.camera_ms = old.detail.camera_ms;
                event.id = old.id.clone();
                event.label = old.label.clone();
                event.time_ms = old.time_ms;
                event.status = old.status;
            }
            event
        })
        .collect();
    events.extend(existing.iter().filter(|e| !e.kind.is_extracted()).cloned());
    events.sort_by(|a, b| a.route_m.total_cmp(&b.route_m));
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, kind: EventKind, label: &str, route_m: f64, status: EventStatus) -> Event {
        Event {
            id: id.into(),
            kind,
            label: label.into(),
            route_m,
            time_ms: (status == EventStatus::Confirmed).then_some(1_000),
            status,
            detail: EventDetail::default(),
        }
    }

    fn extracted(kind: EventKind, label: &str, route_m: f64) -> Event {
        event("", kind, label, route_m, EventStatus::Estimated)
    }

    #[test]
    fn a_confirmed_event_keeps_its_time_at_the_new_position() {
        let old = [event(
            "a",
            EventKind::Tunnel,
            "白地トンネル",
            1000.0,
            EventStatus::Confirmed,
        )];
        let new = carry_over(
            &old,
            vec![extracted(EventKind::Tunnel, "白地トンネル", 1150.0)],
        );
        assert_eq!(new.len(), 1);
        assert_eq!((new[0].id.as_str(), new[0].route_m), ("a", 1150.0));
        assert_eq!(
            (new[0].status, new[0].time_ms),
            (EventStatus::Confirmed, Some(1_000))
        );
    }

    #[test]
    fn an_event_too_far_away_is_a_new_one() {
        let old = [event(
            "a",
            EventKind::Tunnel,
            "白地トンネル",
            1000.0,
            EventStatus::Confirmed,
        )];
        let new = carry_over(
            &old,
            vec![extracted(EventKind::Tunnel, "白地トンネル", 1300.0)],
        );
        assert_eq!(
            (new[0].id.as_str(), new[0].status),
            ("", EventStatus::Estimated)
        );
    }

    #[test]
    fn the_kind_and_the_label_must_both_match() {
        let old = [event(
            "a",
            EventKind::Tunnel,
            "白地トンネル",
            1000.0,
            EventStatus::Hidden,
        )];
        let new = carry_over(
            &old,
            vec![
                extracted(EventKind::Bridge, "白地トンネル", 1000.0),
                extracted(EventKind::Tunnel, "別のトンネル", 1000.0),
            ],
        );
        assert!(new.iter().all(|e| e.id.is_empty()));
    }

    #[test]
    fn one_old_event_is_inherited_only_once() {
        let old = [event(
            "a",
            EventKind::Road,
            "国道19号",
            1000.0,
            EventStatus::Hidden,
        )];
        let new = carry_over(
            &old,
            vec![
                extracted(EventKind::Road, "国道19号", 950.0),
                extracted(EventKind::Road, "国道19号", 1100.0),
            ],
        );
        let ids: Vec<_> = new.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["a", ""]);
    }

    #[test]
    fn what_a_person_added_stays_and_what_vanished_from_the_route_goes() {
        let old = [
            event(
                "d",
                EventKind::Departure,
                "長野市役所前 出発",
                0.0,
                EventStatus::Confirmed,
            ),
            event(
                "c",
                EventKind::Custom,
                "休憩",
                5000.0,
                EventStatus::Estimated,
            ),
            event(
                "t",
                EventKind::Tunnel,
                "消えたトンネル",
                2000.0,
                EventStatus::Confirmed,
            ),
        ];
        let new = carry_over(&old, vec![extracted(EventKind::Road, "国道19号", 100.0)]);
        let ids: Vec<_> = new.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["d", "", "c"]);
    }

    #[test]
    fn a_short_bridge_a_person_chose_to_show_stays_shown() {
        let old = [event(
            "a",
            EventKind::Bridge,
            "短い橋",
            1000.0,
            EventStatus::Estimated,
        )];
        let mut short = extracted(EventKind::Bridge, "短い橋", 1000.0);
        short.status = EventStatus::Hidden;
        let new = carry_over(&old, vec![short]);
        assert_eq!(
            (new[0].id.as_str(), new[0].status),
            ("a", EventStatus::Estimated)
        );
    }

    #[test]
    fn a_name_a_person_gave_survives_and_still_matches_next_time() {
        let old = [event(
            "a",
            EventKind::Bridge,
            "",
            1000.0,
            EventStatus::Estimated,
        )];
        let mut named = carry_over(&old, vec![extracted(EventKind::Bridge, "", 1000.0)]);
        named[0].label = "名無しだった橋".into();

        let again = carry_over(&named, vec![extracted(EventKind::Bridge, "", 1050.0)]);
        assert_eq!(
            (
                again[0].id.as_str(),
                again[0].label.as_str(),
                again[0].route_m
            ),
            ("a", "名無しだった橋", 1050.0)
        );
        assert_eq!(again[0].detail.extracted_label.as_deref(), Some(""));
    }

    #[test]
    fn the_camera_time_of_a_confirmed_event_is_kept() {
        let mut old = event(
            "a",
            EventKind::Tunnel,
            "白地トンネル",
            1000.0,
            EventStatus::Confirmed,
        );
        old.detail.camera_ms = Some(900);
        let new = carry_over(
            &[old],
            vec![extracted(EventKind::Tunnel, "白地トンネル", 1010.0)],
        );
        assert_eq!(new[0].detail.camera_ms, Some(900));
    }

    #[test]
    fn the_names_on_disk_are_snake_case() {
        let event = extracted(EventKind::MichiNoEki, "道の駅 しなの", 1.0);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""kind":"michi_no_eki""#), "{json}");
        assert!(json.contains(r#""status":"estimated""#), "{json}");
        assert!(json.contains(r#""detail":{}"#), "{json}");
    }
}
