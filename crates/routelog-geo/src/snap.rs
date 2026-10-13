//! 経由点の間を、国道・都道府県道に沿って結ぶ。
//!
//! 道の線を交点と端点で分けたグラフの上で、最短の道のりを探す。一方通行は見ない。

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use routelog_core::geo::{self, LonLat};

use crate::data::Road;

/// 吸着の閾値。
#[derive(Debug, Clone, PartialEq)]
pub struct SnapOptions {
    /// 経由点からこの距離（m）までの道に吸着する。
    pub road_m: f64,
    /// 道のりが直線距離のこの倍を超えたら、吸着しない。意図しない迂回を防ぐ。
    pub max_ratio: f64,
}

impl Default for SnapOptions {
    fn default() -> Self {
        Self {
            road_m: 30.0,
            max_ratio: 1.5,
        }
    }
}

/// 交点から交点までの道。
struct Edge {
    from: usize,
    to: usize,
    line: Vec<LonLat>,
    /// `line` の各点までの、`from` からの道のり（m）。
    along: Vec<f64>,
}

impl Edge {
    fn length_m(&self) -> f64 {
        self.along.last().copied().unwrap_or(0.0)
    }

    /// 道のり `m` の点。
    fn point(&self, m: f64) -> LonLat {
        let i = self
            .along
            .partition_point(|a| *a <= m)
            .clamp(1, self.line.len() - 1);
        let (a, b) = (self.along[i - 1], self.along[i]);
        let t = if b > a {
            ((m - a) / (b - a)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (p, q) = (self.line[i - 1], self.line[i]);
        [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]
    }

    /// 道のり `from_m` から `to_m` までの線。逆向きにも辿れる。
    fn slice(&self, from_m: f64, to_m: f64) -> Vec<LonLat> {
        let (low, high) = (from_m.min(to_m), from_m.max(to_m));
        let mut line = vec![self.point(low)];
        line.extend(
            self.line
                .iter()
                .zip(&self.along)
                .filter(|(_, m)| low < **m && **m < high)
                .map(|(p, _)| *p),
        );
        line.push(self.point(high));
        if from_m > to_m {
            line.reverse();
        }
        line
    }
}

/// 道の上の 1 点。
#[derive(Debug, Clone, Copy)]
pub struct OnRoad {
    edge: usize,
    /// その道の `from` からの道のり（m）。
    m: f64,
}

/// 吸着に使う道のグラフ。
pub struct Graph {
    edges: Vec<Edge>,
    /// 交点ごとの、そこに繋がる道。
    adjacency: Vec<Vec<usize>>,
    /// `places[道の番号][線分の番号]` が、その線分のある道と、線分の始点の `line` での番号。
    /// 吸着に使わない道では空。
    places: Vec<Vec<(usize, usize)>>,
}

/// 一般の道で、自動車専用でないもの。125cc が走れる道である。
pub fn usable(road: &Road) -> bool {
    road.kind == "road" && !road.motorroad
}

fn key(point: LonLat) -> (u64, u64) {
    (point[0].to_bits(), point[1].to_bits())
}

impl Graph {
    pub fn new(roads: &[Road]) -> Self {
        // 2 本以上の道が通る点と、道の端が交点になる。
        let mut seen: HashMap<(u64, u64), u32> = HashMap::new();
        for road in roads.iter().filter(|r| usable(r) && r.line.len() >= 2) {
            for (i, point) in road.line.iter().enumerate() {
                let end = i == 0 || i == road.line.len() - 1;
                *seen.entry(key(*point)).or_default() += if end { 2 } else { 1 };
            }
        }
        let mut nodes: HashMap<(u64, u64), usize> = HashMap::new();
        let mut node = |point: LonLat| {
            let next = nodes.len();
            *nodes.entry(key(point)).or_insert(next)
        };

        let mut edges: Vec<Edge> = Vec::new();
        let mut places = vec![Vec::new(); roads.len()];
        for (r, road) in roads.iter().enumerate() {
            if !usable(road) || road.line.len() < 2 {
                continue;
            }
            let mut start = 0;
            for i in 1..road.line.len() {
                places[r].push((edges.len(), i - 1 - start));
                if seen[&key(road.line[i])] < 2 {
                    continue;
                }
                let line = road.line[start..=i].to_vec();
                let mut along = vec![0.0];
                for pair in line.windows(2) {
                    along.push(along[along.len() - 1] + geo::distance_m(pair[0], pair[1]));
                }
                edges.push(Edge {
                    from: node(line[0]),
                    to: node(line[line.len() - 1]),
                    line,
                    along,
                });
                start = i;
            }
        }

        let mut adjacency = vec![Vec::new(); nodes.len()];
        for (e, edge) in edges.iter().enumerate() {
            adjacency[edge.from].push(e);
            adjacency[edge.to].push(e);
        }
        Self {
            edges,
            adjacency,
            places,
        }
    }

    /// 道の線分の上の点を、グラフの上の位置にする。`t` は線分の始点からの割合。
    pub fn on_road(&self, road: usize, segment: usize, t: f64) -> Option<OnRoad> {
        let (edge, index) = *self.places.get(road)?.get(segment)?;
        let along = &self.edges[edge].along;
        let m = along[index] + (along[index + 1] - along[index]) * t.clamp(0.0, 1.0);
        Some(OnRoad { edge, m })
    }

    /// `from` から `to` までの最短の道のりの線。`limit_m` を超えるなら `None`。
    pub fn path(&self, from: OnRoad, to: OnRoad, limit_m: f64) -> Option<Vec<LonLat>> {
        let (first, last) = (&self.edges[from.edge], &self.edges[to.edge]);

        // 同じ道の上なら、その道を辿るだけの道のりが候補になる。
        let mut best: Option<(f64, Option<usize>)> =
            (from.edge == to.edge).then_some(((from.m - to.m).abs(), None));

        // 始点のある道の両端から、交点を広げていく。
        let mut cost: HashMap<usize, f64> = HashMap::new();
        let mut came: HashMap<usize, (usize, usize)> = HashMap::new();
        let mut queue = BinaryHeap::new();
        for (node, m) in [(first.from, from.m), (first.to, first.length_m() - from.m)] {
            if m < *cost.get(&node).unwrap_or(&f64::MAX) {
                cost.insert(node, m);
                queue.push(Reached { cost: m, node });
            }
        }
        while let Some(Reached {
            cost: reached,
            node,
        }) = queue.pop()
        {
            if reached > limit_m {
                break;
            }
            if reached > cost[&node] {
                continue;
            }
            for (end, m) in [(last.from, to.m), (last.to, last.length_m() - to.m)] {
                if end == node && best.is_none_or(|(b, _)| reached + m < b) {
                    best = Some((reached + m, Some(node)));
                }
            }
            for &e in &self.adjacency[node] {
                let edge = &self.edges[e];
                let next = if edge.from == node {
                    edge.to
                } else {
                    edge.from
                };
                let total = reached + edge.length_m();
                if total < *cost.get(&next).unwrap_or(&f64::MAX) {
                    cost.insert(next, total);
                    came.insert(next, (node, e));
                    queue.push(Reached {
                        cost: total,
                        node: next,
                    });
                }
            }
        }

        let (total, via) = best?;
        if total > limit_m {
            return None;
        }
        let Some(mut node) = via else {
            return Some(first.slice(from.m, to.m));
        };

        // 終点側から始点側へ、通った道を集める。
        let end_m = if last.from == node {
            0.0
        } else {
            last.length_m()
        };
        let mut pieces = vec![last.slice(end_m, to.m)];
        while let Some(&(previous, e)) = came.get(&node) {
            let edge = &self.edges[e];
            let forward = edge.from == previous;
            let (a, b) = if forward {
                (0.0, edge.length_m())
            } else {
                (edge.length_m(), 0.0)
            };
            pieces.push(edge.slice(a, b));
            node = previous;
        }
        let start_m = if first.from == node {
            0.0
        } else {
            first.length_m()
        };
        pieces.push(first.slice(from.m, start_m));

        let mut line: Vec<LonLat> = Vec::new();
        for piece in pieces.into_iter().rev() {
            for point in piece {
                if line.last() != Some(&point) {
                    line.push(point);
                }
            }
        }
        Some(line)
    }
}

/// 交点と、そこまでの道のり。道のりの短い順に取り出す。
struct Reached {
    cost: f64,
    node: usize,
}

impl PartialEq for Reached {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Reached {}

impl PartialOrd for Reached {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Reached {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then(self.node.cmp(&other.node))
    }
}
