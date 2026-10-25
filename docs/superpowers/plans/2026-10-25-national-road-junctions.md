# 国道の交差点の字幕 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 国道どうしが関わる交差点に、関わる国道をすべて並べた名前の `road` イベントを出す。

**Architecture:** `routelog-geo` の抽出で、走っている道の頂点のうち別の道と座標を共有する点を交差点とみなし、近い交差点をまとめる。路線の組が変わった所（いまの `road`）と交差点を突き合わせ、名前は純粋な関数 `junction::label` で付ける。アプリ側と Worker は変えない。

**Tech Stack:** Rust（routelog-geo、routelog-core）、rstar、cargo test

**Spec:** design.md の「処理フロー」の「国道の交差点の字幕」

## Global Constraints

- 名前の並びは、国道、都道府県道の順に番号の若い順（`RouteRef` の `Ord` と同じ）。区切りは「・」
- 抜けたあとも走る路線は名前だけ。来ているが抜けたあとは走らない国道は「（分岐）」。抜けたあとに走る国道が無いときは、それまで走っていた国道に「（離脱）」
- 都道府県道は走る道としてだけ出す。来ているだけの都道府県道は出さない
- 国道を走っていないとき（前後とも国道が無い）の交差点では出さない
- 種類は `EventKind::Road`。`detail.routes` は抜けたあとに走る路線（いまと同じ意味）
- 短い寄り道（路線の無い道が `road_again_m` 未満で同じ組に戻る）は、離脱も戻りも出さない。`road_noise_m` 未満の切り替わりは無視する（いまと同じ）
- 立体交差（座標を共有しない）は交差点にしない
- 引き継ぎ（`routelog-core` の `carry_over`）は変えない
- 実データの座標・走行の中身をリポジトリに入れない。テストは合成の座標で書く

## Review Focus

- 往復の経路で同じ交差点を 2 度通る：通るたびに 1 件ずつ出る（タスク 2 のテスト `passing_the_same_junction_twice_is_two_events`）
- 升目の境にある交差点：両方の升目に同じ道が入っていても、1 件で、来ている国道も重ならない（タスク 2 の `a_junction_on_the_tile_edge_is_one_event`）
- 同じ道が交差点で 2 本の way に分かれるだけ（国道 19 号の way が切れているだけ）：交差点として数えても、名前の変わらない所には出さない（タスク 2 の `a_way_split_on_the_same_route_is_quiet`）
- 経路の終わりが路線の無い道：最後の離脱が出る（タスク 2 の `leaving_a_national_road_is_a_departure` で、経路を路線の無い道で終える）
- 交差点の直後に都道府県道の重用が始まる：交差点のイベント 1 件にまとまり、名前に都道府県道が入る（タスク 2 の `a_change_near_a_junction_is_one_event`）

---

### Task 1: 名前の付け方（`junction::label`）

**Files:**
- Create: `crates/routelog-geo/src/junction.rs`
- Modify: `crates/routelog-geo/src/lib.rs`（`mod junction;` を足す。公開しない）

**Interfaces:**
- Produces: `pub(crate) fn label(before: &[RouteRef], after: &[RouteRef], touching: &[RouteRef]) -> String`
  - `before`・`after`：交差点の前と後で走る路線（国道と都道府県道）。`touching`：交差点に来ている国道（都道府県道が入っていても無視する）
  - 並べるのは `after` ∪ `before` の国道 ∪ `touching` の国道。重複は 1 つにする

- [ ] **Step 1: 失敗するテストを書く**（`junction.rs` の `#[cfg(test)] mod tests`。`national(n)`・`prefectural(n)` の補助は extract.rs のテストと同じ形で置く）

```rust
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
    assert_eq!(label(&both, &n19, &[national(403)]), "国道19号・国道403号（分岐）");
    assert_eq!(label(&n19, &n19, &[national(19), national(20)]), "国道19号・国道20号（分岐）");
    assert_eq!(label(&n19, &[], &[national(19)]), "国道19号（離脱）");
    assert_eq!(label(&[], &[prefectural(5)], &[]), "長野県道5号");
}

#[test]
fn turning_onto_another_national_road_branches_the_first() {
    assert_eq!(
        label(&[national(19)], &[national(147)], &[national(19), national(147)]),
        "国道19号（分岐）・国道147号"
    );
}

#[test]
fn leaving_marks_only_the_roads_left_and_prefectural_roads_only_when_taken() {
    assert_eq!(
        label(&[national(19)], &[prefectural(5)], &[national(19), national(147), prefectural(12)]),
        "国道19号（離脱）・国道147号（分岐）・長野県道5号"
    );
    // 重なっていた都道府県道が離れても、名前には出さない。
    assert_eq!(label(&[national(19), prefectural(80)], &[national(19)], &[]), "国道19号");
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p routelog-geo junction`
Expected: FAIL（`label` が無い）

- [ ] **Step 3: `label` を実装する**

`BTreeSet<&RouteRef>` に集めて並べ、`routelog_core::route_name` で名前にする。印の規則は Global Constraints のとおり。

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p routelog-geo junction`
Expected: PASS（4 件）

- [ ] **Step 5: コミット**

---

### Task 2: 交差点を見つけ、`road` を作り直す

**Files:**
- Modify: `crates/routelog-geo/src/extract.rs`（`Options`、`Prepared`、`road_events`、`extract_samples`、テスト）
- Modify: `crates/routelog-geo/src/junction.rs`（交差点の型とまとめ方）

**Interfaces:**
- Consumes: `junction::label`（タスク 1）
- Produces（crate の中だけ）:
  - `Options` に `pub junction_m: f64`（既定 15.0。経路から交差点の頂点までの距離の上限）と `pub junction_merge_m: f64`（既定 50.0。経路の上でこの距離以内の交差点を 1 つにまとめる）を足す
  - `Prepared` に、吸着に使える道（`snap::usable`）の頂点の座標ごとの道の番号の索引を、`OnceLock` で初めて使うときに作るメソッド `fn vertex_roads(&self) -> &HashMap<(u64, u64), Vec<usize>>`。座標のキーは `snap` と同じく `to_bits`
  - `junction.rs` に `pub(crate) struct Junction { pub route_m: f64, pub touching: Vec<RouteRef> }` と `pub(crate) fn merge(points: Vec<Junction>, merge_m: f64) -> Vec<Junction>`（`route_m` の順に並べ、前の交差点から `merge_m` 以内なら来ている国道を合わせる。位置は最初のもの）

交差点の見つけ方：

1. 走っている道（`matched` に入った道）を道の ID で重複なく集め、それぞれについて、その道を走っていた点の番号を覚える
2. その道の頂点のうち、`vertex_roads` で 2 本以上の道が持つ座標を交差点の候補にする
3. 候補ごとに、覚えた点のうち候補から `junction_m` 以内のものを、番号の続く組に分ける。組ごとに、候補にいちばん近い点の `route_m` に `Junction` を置く。同じ道の上で折り返す往復でも、通るたびに組が分かれる。`touching` は、その座標を持つ道の国道（`kind == "road"` のもの）
4. 升目は点ごとに `tiles[i]` から引く。升目の境の道は両方の升目にあるので、同じ `route_m` の重複は `merge` でまとまる

`road_events` の組み立て：

- 路線の組の求め方（`absorb_short` まで）は変えない
- 組が変わる所を「変化」とする。前の組 `before` と後の組 `after` を持つ
  - 国道を含む組から空の組に変わる所も変化にする（離脱）。ただし、空の範囲が `road_again_m` 未満で、次の組が前と同じなら、離脱も戻りも作らない
  - それ以外の空の範囲の扱い（戻ったときに作るか）は、いまの `last` の決まりのまま
- 変化から `road_noise_m` 以内に交差点があれば、交差点の位置に 1 件だけ作り、名前は `label(before, after, touching)`
- 変化と組にならない交差点は、交差点の `junction_merge_m` 手前と先の点の組を `before`・`after` とし、次をすべて満たすときだけ作る
  - `before` か `after` に国道がある
  - `touching` の国道のうち `after` に無いものがある
- 交差点と組にならない変化は、`touching` を空にして作る
- 同じ位置に 2 件作らない。直前に作ったイベントと同じ名前なら作らない

- [ ] **Step 1: 失敗するテストを書く**（extract.rs の `tests`。交差点の道は、経路の道と同じ座標の頂点を持たせる。経路の道は交差点で way を分けるか、頂点を持たせる）

```rust
// 19 号と 403 号を走ってまっすぐ抜け、147 号が南へ、254 号が北へ起点として分かれる。
#[test]
fn national_roads_starting_at_a_junction_are_branches() {
    let both = vec![national(19), national(403)];
    let events = along(roads(vec![
        road(1, vec![at(0.0, 0.0), at(1000.0, 0.0)], both.clone()),
        road(2, vec![at(1000.0, 0.0), at(2000.0, 0.0)], both.clone()),
        road(3, vec![at(1000.0, 0.0), at(1000.0, -800.0)], vec![national(147)]),
        road(4, vec![at(1000.0, 0.0), at(1000.0, 800.0)], vec![national(254)]),
    ]), 2000.0);
    assert_eq!(summary(&events, EventKind::Road), [
        ("国道19号・国道403号".to_owned(), 0),
        ("国道19号・国道147号（分岐）・国道254号（分岐）・国道403号".to_owned(), 1000),
    ]);
    assert_eq!(events.iter().find(|e| e.route_m > 900.0 && e.kind == EventKind::Road).unwrap().detail.routes, both);
}
```

ほかに、次の名前のテストを同じ形で足す。期待値は design.md の表のとおり。

| テスト | 道の置き方 | 期待する `road` |
| --- | --- | --- |
| `a_national_road_joining_is_named_without_a_mark` | 0〜1000 が 19、1000〜2000 が 19・403、403 の way が北から (1000,0) に来る | (国道19号, 0)、(国道19号・国道403号, 1000) |
| `an_overlapping_national_road_leaving_is_a_branch` | 0〜1000 が 19・403、1000〜2000 が 19、403 が (1000,0) から北へ | (国道19号・国道403号, 0)、(国道19号・国道403号（分岐）, 1000) |
| `a_national_road_crossing_through_is_a_branch` | 19 が (1000,0) に頂点を持ち、20 が (1000,-800)-(1000,0)-(1000,800) | (国道19号, 0)、(国道19号・国道20号（分岐）, 1000) |
| `an_overpass_is_not_a_junction` | 20 が (1000,-800)-(1000,800) で、19 と頂点を共有しない | (国道19号, 0) だけ |
| `leaving_a_national_road_is_a_departure` | 0〜1000 が 19、1000〜3000 が路線の無い道。経路は 3000 で終わる | (国道19号, 0)、(国道19号（離脱）, 1000) |
| `a_short_detour_off_a_national_road_is_quiet` | 19、500 m の路線の無い道、19 | (国道19号, 0) だけ |
| `turning_from_one_national_road_to_another_branches_the_first` | 19 が (0,0)-(1000,0)-(2000,0)、147 が (1000,0)-(1000,1000)。経路は (0,0)→(1000,0)→(1000,1000) | (国道19号, 0)、(国道19号（分岐）・国道147号, 1000) |
| `a_prefectural_road_meeting_is_not_named` | 19 に、県道 12 の way が (1000,0) から北へ | (国道19号, 0) だけ |
| `entering_a_prefectural_road_names_only_it` | 0〜1000 が路線の無い道、1000〜2000 が県道 5 | (長野県道5号, 1000) |
| `a_junction_split_over_nearby_nodes_is_one_event` | 19 が (1000,0) と (1030,0) に頂点、147 が (1000,0) から南へ、254 が (1030,0) から北へ | (国道19号, 0)、(国道19号・国道147号（分岐）・国道254号（分岐）, 1000) |
| `crossing_a_national_road_off_it_is_quiet` | 路線の無い道が (1000,0) に頂点、19 が (1000,-800)-(1000,0)-(1000,800) | なし |
| `passing_the_same_junction_twice_is_two_events` | 19 が (0,0)-(1000,0)-(2000,0)、147 が (1000,0) から南へ。経路は (0,0)→(2000,0)→(0,0) | 1000 と 3000 に「国道19号・国道147号（分岐）」 |
| `a_junction_on_the_tile_edge_is_one_event` | `across_edge` の補助で、交差点の道を西と東の両方の升目に入れる | 交差点の `road` が 1 件 |
| `a_way_split_on_the_same_route_is_quiet` | 19 の way が (1000,0) で 2 本に分かれるだけ | (国道19号, 0) だけ |
| `a_change_near_a_junction_is_one_event` | 147 が (1000,0) から南へ、1040〜2000 が 19・県道 80 | (国道19号, 0)、(国道19号・国道147号（分岐）・長野県道80号, 1000) |

既存のテスト（`a_change_of_routes_is_an_event_with_every_route_on_the_road` など）は、期待値を変えずに通ること。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p routelog-geo`
Expected: 新しいテストが FAIL、既存のテストは PASS

- [ ] **Step 3: 上の「交差点の見つけ方」と「`road_events` の組み立て」を実装する**

`road_events` の引数に `samples`・`tiles`・`matched`・`options` を渡し、`extract_samples` から呼ぶ。`Options` の既定に 2 つの値を足す。

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p routelog-geo` と `cargo clippy -p routelog-geo --all-targets`
Expected: すべて PASS、警告なし

- [ ] **Step 5: コミット**

---

### Task 3: 実データで確かめ、設計書を直し、リリースビルドを作る

**Files:**
- Create: `crates/routelog-geo/examples/ride_roads.rs`
- Modify: `crates/routelog-geo/Cargo.toml`（`[dev-dependencies]` に `serde_json.workspace = true`）
- Modify: `design.md`

**Interfaces:**
- Consumes: `routelog_geo::Tiles::extract`、`routelog_core::RideDocument::line`

- [ ] **Step 1: 例を書く**

使い方：`cargo run --release -p routelog-geo --example ride_roads -- <判定用データのフォルダ> <走行の JSON> <始めの km> <終わりの km>`。走行の JSON を `RideDocument` として読み、`doc.line()` を `Tiles::extract` に渡し、範囲の `road` を「km 名前」で出す。走行のファイルにもデータのフォルダにも書かない。

- [ ] **Step 2: 受け入れ条件を確かめる**

判定用データのフォルダは、アプリのデータフォルダの `config.json` の `geo_data_dir`。走行は同じデータフォルダの `rides/` にある 2026-10-03 の走行（読むだけ。座標をどこにも写さない）。

Run: 上のコマンドを 60〜75 km で
Expected: 65.8〜71.8 km に「国道19号・国道147号（分岐）・国道254号（分岐）・国道403号」が 1 件。割れたり国道が抜けたりしたら、`junction_merge_m` と `junction_m` を見直し、合成のテストで再現してからタスク 2 の実装を直す

- [ ] **Step 3: 設計書を直す**

「国道の交差点の字幕」の見出しを「2026-10-25 に実装」にし、見つけ方（頂点の共有、`junction_m`・`junction_merge_m` の値、まとめ方）と、Step 2 で出た名前と件数を書く。決定事項に 1 行足す。

- [ ] **Step 4: 全体を確かめる**

Run: `cargo test --workspace`、`cargo clippy --workspace --all-targets`、`cargo fmt --check`、`app/` で `mise exec -- bun test`
Expected: すべて PASS

- [ ] **Step 5: リリースビルドを作る**

Run: `app/` で `mise exec -- bun run tauri build`
Expected: `target/release/routelog.exe` ができる（利用者はリリースビルドのアプリを閉じてある）

- [ ] **Step 6: コミット**
