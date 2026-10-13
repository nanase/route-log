//! 都道府県道の所属（国道マップの地域名）と、都道府県の名前。

use routelog_core::{RouteClass, RouteRef};

const NAMES: [(&str, &str); 47] = [
    ("aichi", "愛知県"),
    ("akita", "秋田県"),
    ("aomori", "青森県"),
    ("chiba", "千葉県"),
    ("ehime", "愛媛県"),
    ("fukui", "福井県"),
    ("fukuoka", "福岡県"),
    ("fukushima", "福島県"),
    ("gifu", "岐阜県"),
    ("gunma", "群馬県"),
    ("hiroshima", "広島県"),
    ("hokkaido", "北海道"),
    ("hyogo", "兵庫県"),
    ("ibaraki", "茨城県"),
    ("ishikawa", "石川県"),
    ("iwate", "岩手県"),
    ("kagawa", "香川県"),
    ("kagoshima", "鹿児島県"),
    ("kanagawa", "神奈川県"),
    ("kochi", "高知県"),
    ("kumamoto", "熊本県"),
    ("kyoto", "京都府"),
    ("mie", "三重県"),
    ("miyagi", "宮城県"),
    ("miyazaki", "宮崎県"),
    ("nagano", "長野県"),
    ("nagasaki", "長崎県"),
    ("nara", "奈良県"),
    ("niigata", "新潟県"),
    ("oita", "大分県"),
    ("okayama", "岡山県"),
    ("okinawa", "沖縄県"),
    ("osaka", "大阪府"),
    ("saga", "佐賀県"),
    ("saitama", "埼玉県"),
    ("shiga", "滋賀県"),
    ("shimane", "島根県"),
    ("shizuoka", "静岡県"),
    ("tochigi", "栃木県"),
    ("tokushima", "徳島県"),
    ("tokyo", "東京都"),
    ("tottori", "鳥取県"),
    ("toyama", "富山県"),
    ("wakayama", "和歌山県"),
    ("yamagata", "山形県"),
    ("yamaguchi", "山口県"),
    ("yamanashi", "山梨県"),
];

/// 路線の呼び名。「国道19号」「長野県道80号」「北海道道1号」。
pub fn route_name(route: &RouteRef) -> String {
    match route.class {
        RouteClass::National => format!("国道{}号", route.number),
        RouteClass::Prefectural => {
            let name = NAMES
                .iter()
                .find(|(slug, _)| *slug == route.pref)
                .map_or(route.pref.as_str(), |(_, name)| name);
            format!("{name}道{}号", route.number)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefectural(pref: &str, number: u32) -> RouteRef {
        RouteRef {
            class: RouteClass::Prefectural,
            pref: pref.into(),
            number,
        }
    }

    #[test]
    fn names_follow_the_kind_of_prefecture() {
        assert_eq!(route_name(&prefectural("nagano", 80)), "長野県道80号");
        assert_eq!(route_name(&prefectural("hokkaido", 1)), "北海道道1号");
        assert_eq!(route_name(&prefectural("tokyo", 318)), "東京都道318号");
        assert_eq!(route_name(&prefectural("osaka", 2)), "大阪府道2号");
        let national = RouteRef {
            class: RouteClass::National,
            pref: String::new(),
            number: 19,
        };
        assert_eq!(route_name(&national), "国道19号");
    }
}
