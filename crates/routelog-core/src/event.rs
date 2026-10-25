//! イベント。経路の上の位置と、そこを通った時刻を持つ。字幕にするものと、時刻の基準に
//! するだけで字幕にしないものがある。

use serde::{Deserialize, Serialize};

use crate::geo::LonLat;

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
    /// 経由点。人が時刻を入れたいと選んだ経由点にだけ作る。
    Waypoint,
    /// 停車。始まりの時刻と長さを持ち、その間は位置が変わらないものとして時刻を推定する。
    Stop,
    Custom,
}

impl EventKind {
    /// 経路から機械的に抽出する種類。残りは、経路の端と経由点に置くものと、人が足すもの。
    pub fn is_extracted(self) -> bool {
        !matches!(
            self,
            Self::Departure | Self::Arrival | Self::Waypoint | Self::Stop | Self::Custom
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    /// 時刻は推定。
    Estimated,
    /// 時刻を人が確定した。
    Confirmed,
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
    /// `waypoint`：その経由点の位置。経路を直しても、同じ経由点だと分かる。
    /// `stop`：地図で決めた停車の場所。あれば `by_time` は偽で、着いた時刻と出た時刻の基準になる。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<LonLat>,
    /// `stop`：停車していた長さ（ミリ秒）。終わりの時刻は、通過時刻にこれを足したもの。
    ///
    /// 終わりの時刻ではなく長さで持つので、時計の補正値を変えても、長さは変わらない。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stay_ms: Option<i64>,
    /// `bridge`・`tunnel`：国土交通省のデータベースで対応づけた施設の名前。何件かをまとめたときは
    /// 「・」で繋ぐ。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub official_name: Option<String>,
    /// `bridge`・`tunnel`：対応づけた施設の公称の長さ（m）。何件かをまとめたときは足したもの。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub official_length_m: Option<f64>,
    /// 人が字幕にするかどうかを切り替えた。抽出し直しても、橋・トンネルの字幕の有無を決め直さない。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden_by_hand: bool,
    /// 人が名前を直したか、名前の候補から選んだ。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub named_by_hand: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "Stored")]
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
    /// 字幕にしない。時刻を確定してあれば、字幕にしなくても時刻推定の基準になる。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    #[serde(default)]
    pub detail: EventDetail,
}

impl Event {
    /// 抽出したときの名前。無ければ、いまの名前。
    fn extracted_name(&self) -> &str {
        self.detail
            .extracted_label
            .as_deref()
            .unwrap_or(&self.label)
    }

    /// 人が名前を直したか、名前の候補から選んだ。
    pub fn renamed(&self) -> bool {
        self.detail.named_by_hand || self.label != self.extracted_name()
    }

    /// 一覧とマーカーに出す名前。
    ///
    /// 橋・トンネルは、国土交通省のデータベースの名前と公称の長さを使い、「犬戻トンネル 737m」の
    /// 形にする。OSM の名前と違えば、人が選ぶまで「OSM の名前／公称の名前」と両方を並べる。
    pub fn shown_name(&self) -> String {
        if !matches!(self.kind, EventKind::Bridge | EventKind::Tunnel) {
            return self.label.clone();
        }
        let name = match self.detail.official_name.as_deref() {
            Some(official) if !self.renamed() => {
                if self.label.is_empty() || same_name(&self.label, official) {
                    official.to_owned()
                } else {
                    format!("{}／{official}", self.label)
                }
            }
            _ => self.label.clone(),
        };
        match self.detail.official_length_m {
            Some(length_m) if !name.is_empty() && !ends_with_length(&name) => {
                format!("{name} {}m", length(length_m))
            }
            _ => name,
        }
    }
}

/// 長さの数。小数があれば 1 桁、無ければ整数にする。
fn length(length_m: f64) -> String {
    let tenths = (length_m * 10.0).round() as i64;
    if tenths % 10 == 0 {
        (tenths / 10).to_string()
    } else {
        format!("{:.1}", tenths as f64 / 10.0)
    }
}

/// 「737m」「291.3m」のような長さで終わるか。
fn ends_with_length(name: &str) -> bool {
    let Some(rest) = name.strip_suffix('m') else {
        return false;
    };
    let digits = rest
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .count();
    digits > 0 && rest[..rest.len() - digits].ends_with(' ')
}

/// 字幕で区別しない揺れ（全角と半角、ケ・ヶ・ヵ・ケ）を揃えた名前。
fn loose(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '\u{3000}' => ' ',
            'ヶ' | 'ヵ' | 'ケ' => 'ケ',
            _ => c,
        })
        .collect()
}

/// 字幕で区別しない揺れだけが違う、同じ名前か。
pub fn same_name(a: &str, b: &str) -> bool {
    loose(a) == loose(b)
}

/// 保存してあるイベントの形。
///
/// 以前は「字幕にしない」を状態の 1 つとして持っていた。その形で保存したものも読めるよう、
/// ここで受けて、いまの形に直す。
#[derive(Deserialize)]
struct Stored {
    id: String,
    kind: EventKind,
    label: String,
    route_m: f64,
    time_ms: Option<i64>,
    status: StoredStatus,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    detail: EventDetail,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredStatus {
    Estimated,
    Confirmed,
    Hidden,
}

impl From<Stored> for Event {
    fn from(stored: Stored) -> Self {
        Self {
            id: stored.id,
            kind: stored.kind,
            label: stored.label,
            route_m: stored.route_m,
            time_ms: stored.time_ms,
            status: match stored.status {
                StoredStatus::Confirmed => EventStatus::Confirmed,
                StoredStatus::Estimated | StoredStatus::Hidden => EventStatus::Estimated,
            },
            hidden: stored.hidden || stored.status == StoredStatus::Hidden,
            detail: stored.detail,
        }
    }
}

/// 経路から抽出し直したイベントに、人が手を入れた結果を引き継ぐ。
///
/// - 抽出したイベントは、同じ種類で、抽出したときの名前が同じで、位置が近い元のイベントが
///   あれば、その ID・状態・時刻・字幕にするかどうか・人が直した名前を受け継ぐ。位置と公称の
///   名前・長さは新しいものにする
/// - 橋・トンネルの字幕にするかどうかは、人が切り替えたものだけを受け継ぐ。ほかは抽出で決め直す
/// - 抽出しないイベント（出発・到着・経由点・手動）は、そのまま残す
/// - 受け継ぐ相手のいない元のイベントは消える
/// - 名前の無い橋は、人が名前を付けておらず、公称の名前も無ければ捨てる。字幕にできず、一覧を
///   埋めるだけになる
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
                if old.renamed() {
                    event.label = old.label.clone();
                    event.detail.named_by_hand = old.detail.named_by_hand;
                }
                event.time_ms = old.time_ms;
                event.status = old.status;
                // 橋・トンネルの字幕の有無は、公称の長さが後から届くことがあるので、人が切り替えて
                // いなければ決め直す。
                let decided = matches!(event.kind, EventKind::Bridge | EventKind::Tunnel);
                if !decided || old.detail.hidden_by_hand {
                    event.hidden = old.hidden;
                    event.detail.hidden_by_hand = old.detail.hidden_by_hand;
                }
            }
            event
        })
        .filter(|event| {
            event.kind != EventKind::Bridge
                || !event.label.is_empty()
                || event.detail.official_name.is_some()
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
            hidden: false,
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
            EventStatus::Confirmed,
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
            EventStatus::Confirmed,
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
        let mut shown = event(
            "a",
            EventKind::Bridge,
            "短い橋",
            1000.0,
            EventStatus::Estimated,
        );
        shown.detail.hidden_by_hand = true;
        let old = [shown];
        let mut short = extracted(EventKind::Bridge, "短い橋", 1000.0);
        short.hidden = true;
        let new = carry_over(&old, vec![short]);
        assert_eq!((new[0].id.as_str(), new[0].hidden), ("a", false));
    }

    #[test]
    fn an_event_a_person_hid_stays_hidden_and_keeps_its_time() {
        let mut old = event(
            "a",
            EventKind::Tunnel,
            "白地トンネル",
            1000.0,
            EventStatus::Confirmed,
        );
        old.hidden = true;
        old.detail.hidden_by_hand = true;
        let new = carry_over(
            &[old],
            vec![extracted(EventKind::Tunnel, "白地トンネル", 1010.0)],
        );
        assert_eq!(
            (new[0].hidden, new[0].status, new[0].time_ms),
            (true, EventStatus::Confirmed, Some(1_000))
        );
    }

    #[test]
    fn a_bridge_without_a_name_is_dropped_unless_a_person_named_it() {
        let mut named = event(
            "a",
            EventKind::Bridge,
            "名無しだった橋",
            1000.0,
            EventStatus::Estimated,
        );
        named.detail.extracted_label = Some(String::new());
        let new = carry_over(
            &[named],
            vec![
                extracted(EventKind::Bridge, "", 1000.0),
                extracted(EventKind::Bridge, "", 5000.0),
                extracted(EventKind::Tunnel, "", 6000.0),
            ],
        );
        let kept: Vec<_> = new
            .iter()
            .map(|e| (e.kind, e.label.as_str(), e.route_m))
            .collect();
        assert_eq!(
            kept,
            [
                (EventKind::Bridge, "名無しだった橋", 1000.0),
                (EventKind::Tunnel, "", 6000.0)
            ]
        );
    }

    #[test]
    fn a_name_a_person_gave_survives_and_still_matches_next_time() {
        let old = [event(
            "a",
            EventKind::Tunnel,
            "",
            1000.0,
            EventStatus::Estimated,
        )];
        let mut named = carry_over(&old, vec![extracted(EventKind::Tunnel, "", 1000.0)]);
        named[0].label = "名無しだったトンネル".into();

        let again = carry_over(&named, vec![extracted(EventKind::Tunnel, "", 1050.0)]);
        assert_eq!(
            (
                again[0].id.as_str(),
                again[0].label.as_str(),
                again[0].route_m
            ),
            ("a", "名無しだったトンネル", 1050.0)
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
        assert!(!json.contains("hidden"), "{json}");
    }

    #[test]
    fn an_event_saved_with_the_hidden_status_is_read_as_hidden() {
        let old = r#"{"id":"a","kind":"bridge","label":"小さな橋","route_m":1.0,"time_ms":null,"status":"hidden"}"#;
        let event: Event = serde_json::from_str(old).unwrap();
        assert_eq!((event.status, event.hidden), (EventStatus::Estimated, true));

        // いまの形は、確定していても字幕にしないことを持てる。書いて読んでも変わらない。
        let mut timed = extracted(EventKind::Waypoint, "経由地 2", 1.0);
        timed.status = EventStatus::Confirmed;
        timed.hidden = true;
        timed.detail.at = Some([138.5, 36.5]);
        let json = serde_json::to_string(&timed).unwrap();
        assert!(json.contains(r#""hidden":true"#), "{json}");
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), timed);
    }

    fn with_official(mut e: Event, name: &str, length_m: f64) -> Event {
        e.detail.extracted_label = Some(e.label.clone());
        e.detail.official_name = Some(name.into());
        e.detail.official_length_m = Some(length_m);
        e
    }

    #[test]
    fn a_renamed_event_keeps_its_name_and_time() {
        let mut old = event(
            "a",
            EventKind::Tunnel,
            "山吹トンネル",
            1000.0,
            EventStatus::Confirmed,
        );
        old.detail.extracted_label = Some(String::new());
        let new = carry_over(
            &[old],
            vec![with_official(
                extracted(EventKind::Tunnel, "", 1010.0),
                "池沢隧道",
                379.0,
            )],
        );
        assert_eq!(new.len(), 1);
        assert_eq!(
            (new[0].id.as_str(), new[0].label.as_str()),
            ("a", "山吹トンネル")
        );
        assert_eq!(new[0].status, EventStatus::Confirmed);
        assert_eq!(new[0].detail.official_name.as_deref(), Some("池沢隧道"));
        assert_eq!(new[0].shown_name(), "山吹トンネル 379m");
    }

    #[test]
    fn an_untouched_unnamed_tunnel_takes_the_new_name_and_keeps_its_time() {
        let mut old = event("a", EventKind::Tunnel, "", 1000.0, EventStatus::Confirmed);
        old.detail.extracted_label = Some(String::new());
        let new = carry_over(
            &[old],
            vec![with_official(
                extracted(EventKind::Tunnel, "", 1010.0),
                "池沢隧道",
                379.0,
            )],
        );
        assert_eq!((new[0].id.as_str(), new[0].time_ms), ("a", Some(1_000)));
        assert_eq!(new[0].shown_name(), "池沢隧道 379m");
    }

    #[test]
    fn a_name_chosen_by_hand_stays_chosen() {
        let mut old = event(
            "a",
            EventKind::Tunnel,
            "新鳥居トンネル",
            1000.0,
            EventStatus::Confirmed,
        );
        old.detail.named_by_hand = true;
        let new = carry_over(
            &[old],
            vec![with_official(
                extracted(EventKind::Tunnel, "新鳥居トンネル", 1000.0),
                "鳥居トンネル",
                1738.0,
            )],
        );
        assert!(new[0].detail.named_by_hand);
        assert_eq!(new[0].shown_name(), "新鳥居トンネル 1738m");
    }

    #[test]
    fn a_bridge_is_shown_again_by_the_new_rule_unless_switched_by_hand() {
        let mut old = event(
            "a",
            EventKind::Bridge,
            "明治橋",
            1000.0,
            EventStatus::Estimated,
        );
        old.hidden = true;
        let new = carry_over(
            &[old.clone()],
            vec![extracted(EventKind::Bridge, "明治橋", 1000.0)],
        );
        assert!(!new[0].hidden);
        old.detail.hidden_by_hand = true;
        let new = carry_over(&[old], vec![extracted(EventKind::Bridge, "明治橋", 1000.0)]);
        assert!(new[0].hidden && new[0].detail.hidden_by_hand);
    }

    #[test]
    fn a_road_event_still_keeps_its_hidden() {
        let mut old = event(
            "a",
            EventKind::Road,
            "国道19号",
            1000.0,
            EventStatus::Estimated,
        );
        old.hidden = true;
        let new = carry_over(&[old], vec![extracted(EventKind::Road, "国道19号", 1000.0)]);
        assert!(new[0].hidden);
    }

    #[test]
    fn an_unnamed_bridge_with_an_official_name_is_kept() {
        let new = carry_over(
            &[],
            vec![
                with_official(extracted(EventKind::Bridge, "", 1000.0), "元橋", 212.0),
                extracted(EventKind::Bridge, "", 2000.0),
            ],
        );
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].shown_name(), "元橋 212m");
    }

    fn structure(kind: EventKind, label: &str) -> Event {
        let mut e = extracted(kind, label, 0.0);
        e.detail.extracted_label = Some(label.into());
        e
    }

    #[test]
    fn a_structure_shows_its_official_length() {
        let mut e = structure(EventKind::Tunnel, "犬戻トンネル");
        e.detail.official_name = Some("犬戻トンネル".into());
        e.detail.official_length_m = Some(737.0);
        assert_eq!(e.shown_name(), "犬戻トンネル 737m");
        e.detail.official_length_m = Some(291.3);
        assert_eq!(e.shown_name(), "犬戻トンネル 291.3m");
    }

    #[test]
    fn an_unnamed_structure_takes_the_official_name() {
        let mut e = structure(EventKind::Tunnel, "");
        e.detail.official_name = Some("池沢隧道".into());
        e.detail.official_length_m = Some(379.0);
        assert_eq!(e.shown_name(), "池沢隧道 379m");
    }

    #[test]
    fn different_names_are_both_shown_until_one_is_chosen() {
        let mut e = structure(EventKind::Tunnel, "新鳥居トンネル");
        e.detail.official_name = Some("鳥居トンネル".into());
        e.detail.official_length_m = Some(1738.0);
        assert_eq!(e.shown_name(), "新鳥居トンネル／鳥居トンネル 1738m");
        e.detail.named_by_hand = true;
        assert_eq!(e.shown_name(), "新鳥居トンネル 1738m");
    }

    #[test]
    fn names_that_differ_only_in_width_or_small_ke_are_the_same() {
        assert!(same_name("角ケ平トンネル", "角ヶ平トンネル"));
        assert!(same_name("大白川第１トンネル", "大白川第1トンネル"));
        assert!(!same_name("角ケ平隧道", "角ヶ平トンネル"));
        let mut e = structure(EventKind::Tunnel, "角ケ平トンネル");
        e.detail.official_name = Some("角ヶ平トンネル".into());
        assert_eq!(e.shown_name(), "角ヶ平トンネル");
    }

    #[test]
    fn a_name_ending_in_a_length_gets_no_second_length() {
        let mut e = structure(EventKind::Tunnel, "犬戻トンネル 737m");
        e.detail.extracted_label = Some("犬戻トンネル".into());
        e.detail.official_length_m = Some(737.0);
        assert_eq!(e.shown_name(), "犬戻トンネル 737m");
    }

    #[test]
    fn other_kinds_show_their_label() {
        let mut e = structure(EventKind::Road, "国道19号");
        e.detail.official_length_m = Some(10.0);
        assert_eq!(e.shown_name(), "国道19号");
    }

    #[test]
    fn the_new_fields_are_left_out_when_empty() {
        let json = serde_json::to_string(&structure(EventKind::Bridge, "明治橋")).unwrap();
        assert!(
            !json.contains("official") && !json.contains("by_hand"),
            "{json}"
        );
    }
}
