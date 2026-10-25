//! 国道どうしが関わる交差点の、路線のイベントの名前。
//!
//! 交差点を抜けたあとも走る路線は名前だけを書き、来ているが抜けたあとは走らない国道には
//! 「（分岐）」を付ける。抜けたあとに走る国道が無ければ、それまで走っていた国道は「（離脱）」
//! にする。都道府県道は、走る道としてだけ出す。

use std::collections::BTreeSet;

use routelog_core::{RouteClass, RouteRef, route_name};

/// 経路が通る交差点。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Junction {
    /// 経路の始点からの距離（m）。
    pub route_m: f64,
    /// 交差点に来ている国道。番号の若い順で、重ならない。
    pub touching: Vec<RouteRef>,
}

/// 経路の上で、まとめ始めた交差点から `merge_m` 以内にある交差点を 1 つにまとめる。位置は
/// 最初の交差点にする。
///
/// 上下線が分かれた道やずれた交差点は、地図データでは何点かに分かれている。升目の境にある
/// 交差点は、両方の升目から同じ位置に出てくる。交差点の続く市街でも、まとめるのは
/// `merge_m` の幅までである。
pub(crate) fn merge(mut points: Vec<Junction>, merge_m: f64) -> Vec<Junction> {
    points.sort_by(|a, b| a.route_m.total_cmp(&b.route_m));
    let mut merged: Vec<Junction> = Vec::new();
    for point in points {
        match merged.last_mut() {
            Some(junction) if point.route_m - junction.route_m <= merge_m => {
                let all: BTreeSet<RouteRef> =
                    junction.touching.drain(..).chain(point.touching).collect();
                junction.touching = all.into_iter().collect();
            }
            _ => merged.push(point),
        }
    }
    merged
}

/// 交差点の名前。
///
/// `before` と `after` は交差点の前と後で走る路線、`touching` は交差点に来ている路線。
/// `touching` の都道府県道は使わない。
pub(crate) fn label(before: &[RouteRef], after: &[RouteRef], touching: &[RouteRef]) -> String {
    let national = |route: &&RouteRef| route.class == RouteClass::National;
    let leaving = !after.iter().any(|r| national(&r));
    let named: BTreeSet<&RouteRef> = after
        .iter()
        .chain(before.iter().filter(national))
        .chain(touching.iter().filter(national))
        .collect();
    let names: Vec<String> = named
        .into_iter()
        .map(|route| {
            let mark = if after.contains(route) {
                ""
            } else if leaving && before.contains(route) {
                "（離脱）"
            } else {
                "（分岐）"
            };
            format!("{}{mark}", route_name(route))
        })
        .collect();
    names.join("・")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn national(number: u32) -> RouteRef {
        RouteRef {
            class: RouteClass::National,
            pref: String::new(),
            number,
        }
    }

    fn prefectural(number: u32) -> RouteRef {
        RouteRef {
            class: RouteClass::Prefectural,
            pref: "nagano".into(),
            number,
        }
    }

    #[test]
    fn the_roads_taken_are_plain_and_the_others_branch() {
        let both = [national(19), national(403)];
        let touching = [national(19), national(147), national(254), national(403)];
        assert_eq!(
            label(&both, &both, &touching),
            "国道19号・国道147号（分岐）・国道254号（分岐）・国道403号"
        );
    }

    #[test]
    fn each_scene_has_its_name() {
        let n19 = [national(19)];
        let both = [national(19), national(403)];
        assert_eq!(label(&[], &n19, &[]), "国道19号");
        assert_eq!(label(&n19, &both, &[national(403)]), "国道19号・国道403号");
        assert_eq!(
            label(&both, &n19, &[national(403)]),
            "国道19号・国道403号（分岐）"
        );
        assert_eq!(
            label(&n19, &n19, &[national(19), national(20)]),
            "国道19号・国道20号（分岐）"
        );
        assert_eq!(label(&n19, &[], &[national(19)]), "国道19号（離脱）");
        assert_eq!(label(&[], &[prefectural(5)], &[]), "長野県道5号");
    }

    #[test]
    fn turning_onto_another_national_road_branches_the_first() {
        assert_eq!(
            label(
                &[national(19)],
                &[national(147)],
                &[national(19), national(147)]
            ),
            "国道19号（分岐）・国道147号"
        );
    }

    #[test]
    fn leaving_marks_only_the_roads_left_and_prefectural_roads_only_when_taken() {
        assert_eq!(
            label(
                &[national(19)],
                &[prefectural(5)],
                &[national(19), national(147), prefectural(12)]
            ),
            "国道19号（離脱）・国道147号（分岐）・長野県道5号"
        );
        // 重なっていた都道府県道が離れても、名前には出さない。
        assert_eq!(
            label(&[national(19), prefectural(80)], &[national(19)], &[]),
            "国道19号"
        );
    }
}
