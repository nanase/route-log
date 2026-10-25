# 橋・トンネルの公称の長さ 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 抽出した橋・トンネルに、国交省のデータベース（Q地図のタイル）の施設を対応づけ、「犬戻トンネル 737m」の形の名前を一覧とマーカーに出す。

**Architecture:** `routelog-geo` に新しいモジュール `official` を置き、タイルの取得と手元への保存、タイルの読み取り、施設の対応づけを持たせる。アプリ（src-tauri）は、走行を開いたときと経路を保存したときに足りないタイルを裏で取り、抽出のたびに手元のタイルで対応づける。名前の組み立ては `routelog-core` の `Event::shown_name` と、画面の同じ関数が受け持つ。

**Tech Stack:** Rust（routelog-core、routelog-geo、ureq 2、geozero の MVT）、Tauri 2、Svelte 5、bun test

**Spec:** design.md の「地図・道路データ」の「橋・トンネルの公称の長さを取り込む」（上の「公称の長さ」と「突き合わせ」の節も含む）

## Global Constraints

- 名前と長さの形は「犬戻トンネル 737m」（名前、半角の空白、数、`m`）。長さは小数があれば 1 桁（`291.3m`）、無ければ整数（`737m`）
- 名前が違うときに候補を並べる区切りは「／」、何件かをまとめた公称の名前の区切りは「・」
- 名前の揺れを揃えて「同じ」とみなすのは、全角と半角、`ケ`・`ヶ`・`ヵ`・`ケ` の違いだけ。隧道とトンネル、第１と１号は違う名前
- 対応づけでの比べ方は、上に加えて、隧道→トンネル、`第` と `号` を除く、数字を半角にする、を揃える
- 橋は、公称の長さ（無ければ OSM の長さ）が 100 m 以上なら字幕あり。トンネルは覆道・洞門のほか、すべて字幕あり
- 人が字幕あり・なしを切り替えたもの（`hidden_by_hand`）は、抽出し直しても決め直さない
- イベントの `label` は OSM の名前のまま。公称の名前と長さは `detail` に持つ
- タイルの User-Agent は `route-log/<版>`。取るのは 1 秒に 20 枚まで。ズーム 13。施設の無いタイル（404）も覚える
- TileJSON は `https://mapdata.qchizu.jp/m56001/2024-tunnel.tilejson` と `https://mapdata.qchizu.jp/m56001/2024-bridge_detail.tilejson`。タイルの URL は TileJSON の `tiles[0]` から引く。ソースレイヤは `tunnel` と `bridge`
- 項目の名前：`施設名`、`路線名`、トンネルは `延長（ｍ）`、橋は `橋長（ｍ）`（数か文字列）
- 実データの座標・タイルをリポジトリに入れない。テストは合成のデータで書く

## Review Focus

- 往復で同じ橋・トンネルを 2 度通る：どちらのイベントにも同じ施設が付く（タスク 4 の `both_passes_get_the_same_facility`）
- 同じ施設がデータベースに 2 件並んでいる（新久米路トンネル 257 m が 2 つ）：まとめても長さを足さない（タスク 4 の `a_duplicated_facility_is_not_added_twice`）
- 通信できない・タイルが壊れている：抽出は止まらず、名前と長さが付かないだけ（タスク 3 の `a_broken_tile_is_skipped`、タスク 5 のエラーを返さないこと）
- 人が名前を直した確定済みのトンネル：抽出し直しても、名前と確定した時刻が残る（タスク 2 の `a_renamed_event_keeps_its_name_and_time`）
- 名前がすでに長さで終わっている（人が「犬戻トンネル 737m」と入れていた）：長さを 2 度付けない（タスク 1 の `a_name_ending_in_a_length_gets_no_second_length`）

---

### Task 1: 名前の組み立てと、イベントの新しい項目（routelog-core）

**Files:**
- Modify: `crates/routelog-core/src/event.rs`（`EventDetail`、`Event::shown_name`、テスト）
- Modify: `crates/routelog-core/src/resolve.rs`（マーカーの名前）
- Modify: `crates/routelog-core/src/export.rs`（CSV の `label` の列）

**Interfaces:**
- Produces:
  - `EventDetail` に `official_name: Option<String>`、`official_length_m: Option<f64>`、`hidden_by_hand: bool`、`named_by_hand: bool`。どれも `#[serde(default, skip_serializing_if = …)]`
  - `pub fn same_name(a: &str, b: &str) -> bool`（Global Constraints の「同じ」の揃え方）
  - `impl Event { pub fn renamed(&self) -> bool }`：`named_by_hand` か、`label` が抽出したときの名前と違う
  - `impl Event { pub fn shown_name(&self) -> String }`：
    - 橋・トンネル以外と、公称の名前も長さも無いものは `label`
    - 人が直した名前があれば `label`。無ければ、`label` が空なら公称の名前、`same_name` なら公称の名前、違えば「`label`／公称の名前」
    - 公称の長さがあり、名前が `\d+(\.\d+)?m` で終わっていなければ、「 737m」を足す
  - マーカーの名前と CSV の `label` は `shown_name`（空なら今までどおり種類の呼び名）

- [ ] **Step 1: 失敗するテストを書く**（event.rs の `tests`）

```rust
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
fn the_new_fields_are_left_out_when_empty() {
    let json = serde_json::to_string(&structure(EventKind::Bridge, "明治橋")).unwrap();
    assert!(!json.contains("official") && !json.contains("by_hand"));
}
```

resolve.rs と export.rs には、公称の長さのある橋が「明治橋 146m」の名前で出るテストを 1 つずつ足す。

- [ ] **Step 2: 失敗を確かめる** — Run: `cargo test -p routelog-core` / Expected: 新しいテストが FAIL（コンパイルエラーを含む）
- [ ] **Step 3: 実装する**（上の Interfaces どおり。長さの書き方は Global Constraints）
- [ ] **Step 4: 通ることを確かめる** — Run: `cargo test -p routelog-core` / Expected: PASS
- [ ] **Step 5: コミット**

---

### Task 2: 引き継ぎ（`carry_over`）

**Files:**
- Modify: `crates/routelog-core/src/event.rs`（`carry_over`、テスト）

**Interfaces:**
- Consumes: タスク 1 の `renamed`、新しい項目
- Produces: `carry_over(existing, extracted)` の決まりを次に変える（引数と戻り値は同じ）
  - 照らし合わせる名前は、新しいイベントの `detail.extracted_label`（無ければ `label`）と、古いイベントの抽出したときの名前
  - 古いイベントが `renamed` なら `label` と `named_by_hand` を受け継ぐ。そうでなければ、新しい `label` を使う。公称の名前と長さは、いつも新しいイベントのもの
  - 橋・トンネルは、古いイベントの `hidden_by_hand` が真のときだけ `hidden` を受け継ぐ（`hidden_by_hand` も受け継ぐ）。偽なら、新しいイベントの `hidden` を使う。ほかの種類は、今までどおり受け継ぐ
  - 名前の無い橋を捨てる決まりは、`label` が空で、公称の名前も無いものにする

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[test]
fn a_renamed_event_keeps_its_name_and_time() { /* 古い: label「山吹トンネル」、extracted_label「」、confirmed。
    新しい: label「」、extracted_label「」、official_name「池沢隧道」。
    結果: id・時刻・status を受け継ぎ、label は「山吹トンネル」、official_name は新しいもの */ }

#[test]
fn an_untouched_unnamed_tunnel_takes_the_new_name_and_keeps_its_time() { /* 古い: label「」、extracted_label「」、confirmed。
    新しい: label「」、official_name「池沢隧道」。結果: shown_name「池沢隧道」、時刻を受け継ぐ */ }

#[test]
fn a_bridge_is_shown_again_by_the_new_rule_unless_switched_by_hand() { /* 古い: hidden 真、hidden_by_hand 偽。
    新しい: hidden 偽。結果: hidden 偽。古いが hidden_by_hand 真なら hidden 真のまま */ }

#[test]
fn a_road_event_still_keeps_its_hidden() { /* 種類 road、古い hidden 真、hidden_by_hand 偽 → hidden 真 */ }

#[test]
fn an_unnamed_bridge_with_an_official_name_is_kept() { /* label「」、official_name「元橋」 → 残る */ }
```

既存の `carry_over` のテストは、期待値を変えずに通ること。

- [ ] **Step 2: 失敗を確かめる** — Run: `cargo test -p routelog-core carry` / Expected: 新しいテストが FAIL
- [ ] **Step 3: 実装する**
- [ ] **Step 4: 通ることを確かめる** — Run: `cargo test -p routelog-core` / Expected: PASS
- [ ] **Step 5: コミット**

---

### Task 3: 字幕にする長さの決まりと、タイルの読み取り（routelog-geo）

**Files:**
- Modify: `crates/routelog-geo/src/extract.rs`（`Options`、`structure_events`、テスト）
- Create: `crates/routelog-geo/src/official.rs`（`Facility`、`decode`、`tiles_along`）
- Modify: `crates/routelog-geo/src/lib.rs`、`crates/routelog-geo/Cargo.toml`（geozero に `with-mvt`）

**Interfaces:**
- Produces:
  - `Options` の `structure_min_m` を `bridge_min_m: f64`（既定 100.0）に替える。橋は OSM の長さがこれ未満なら字幕なし。トンネルは覆道だけ字幕なし
  - `pub enum FacilityKind { Bridge, Tunnel }`
  - `pub struct Facility { pub kind: FacilityKind, pub at: LonLat, pub name: String, pub length_m: Option<f64>, pub route: String }`
  - `pub const ZOOM: u8 = 13;`
  - `pub fn decode(bytes: &[u8], kind: FacilityKind, x: u32, y: u32) -> Vec<Facility>`：ソースレイヤ（`tunnel`・`bridge`）の点を経度・緯度にする。読めないタイルは空
  - `pub fn tiles_along(line: &[LonLat], margin_m: f64) -> BTreeSet<(u32, u32)>`：経路の 100 m ごとの点から `margin_m`（呼ぶ側は 150）以内に掛かるズーム 13 のタイル

- [ ] **Step 1: 失敗するテストを書く**
  - extract.rs：`a_short_bridge_and_a_shed_are_hidden` を直す。80 m の橋は字幕なし、120 m の橋は字幕あり、30 m のトンネルは字幕あり、覆道は字幕なし
  - official.rs：`decode` は、テストの中で geozero の MVT の型で 1 点（施設名「犬戻トンネル」、延長 737）のタイルを組み立てて読む（`a_point_becomes_a_facility_at_its_place`）。長さが文字列「291.3」でも読む。`a_broken_tile_is_skipped` は、でたらめなバイト列で空を返す
  - `tiles_along`：タイルの境から 50 m の経路で、隣のタイルも入る
- [ ] **Step 2: 失敗を確かめる** — Run: `cargo test -p routelog-geo` / Expected: 新しいテストが FAIL
- [ ] **Step 3: 実装する**（点のジオメトリは、MoveTo 1 回とジグザグの x・y だけを読めばよい）
- [ ] **Step 4: 通ることを確かめる** — Run: `cargo test -p routelog-geo` / Expected: PASS
- [ ] **Step 5: コミット**

---

### Task 4: 施設の対応づけ（`official::attach`）

**Files:**
- Modify: `crates/routelog-geo/src/official.rs`

**Interfaces:**
- Consumes: タスク 1 の `EventDetail` の項目、タスク 3 の `Facility`
- Produces: `pub fn attach(events: &mut [Event], line: &[LonLat], facilities: &[Facility])`
  - 相手にするのは、橋と、字幕なしでないトンネル（字幕なしのトンネルは覆道）
  - 候補は、同じ種類で、橋・トンネルの範囲（`route_m` から `end_m`）の経路から 100 m 以内にある施設
  - 点数は、名前が合う（対応づけの比べ方で同じ）、路線が合う（その位置の `road` の `detail.routes` の国道・都道府県道の番号が、施設の `路線名` の番号と同じ）、長さの比（短い方÷長い方）、距離で付ける。名前が合うか、長さの比が 0.7 以上で路線が合うものを採る。OSM で名前の無いものは、長さの比 0.7 以上で 50 m 以内でもよい
  - 採った施設のほかに、範囲の中にある名前の違う同じ種類の施設を足すと長さの比が上がるなら、まとめる（名前は「・」で繋ぎ、長さは足す。名前と長さが同じ施設は 1 つと数える）
  - 付けるのは `official_name`・`official_length_m` だけで、`label` は変えない。OSM の名前が空なら、`shown_name` が公称の名前を出す
  - 橋は、公称の長さで `hidden` を決め直す（100 m 未満なら字幕なし）

- [ ] **Step 1: 失敗するテストを書く**（合成の経路と施設で。試しの実例を写す。名前は design.md の受け入れ条件のとおり）
  - `the_nearest_facility_is_not_always_the_one`（大白川 2 号 190 m、第１ 50 m が 5 m、第２ 187 m が 28 m → 第２）
  - `the_route_decides_between_bridges_of_the_same_name`（稲核橋 90 m、市道の稲核橋 48.5 m が 30 m、国道 158 号の稲核橋 92 m が 88 m → 国道の方）
  - `an_unnamed_tunnel_takes_the_official_name`（名前なし 360 m、池沢隧道 379 m が 16 m → official_name「池沢隧道」、label は空のまま）
  - `two_facilities_in_one_tunnel_are_joined`（笹平トンネル 1,140 m、第１ 586 m、第２ 521 m → 「笹平第１トンネル・笹平第２トンネル」1107）
  - `a_differently_named_tunnel_gets_the_official_name_alongside`（新鳥居 1,760 m、鳥居 1,738 m → official_name「鳥居トンネル」、label は「新鳥居トンネル」）
  - `a_duplicated_facility_is_not_added_twice`（新久米路 250 m、257 m が同じ点に 2 つ → 257）
  - `both_passes_get_the_same_facility`（往復の 2 件に同じ施設）
  - `a_bridge_is_shown_by_its_official_length`（OSM 95 m・公称 105 m → 字幕あり、OSM 105 m・公称 92 m → 字幕なし）
  - `a_shed_gets_nothing`（字幕なしのトンネルの 50 m 先にトンネルの施設 → 付かない）
- [ ] **Step 2: 失敗を確かめる** — Run: `cargo test -p routelog-geo official` / Expected: FAIL
- [ ] **Step 3: 実装する**
- [ ] **Step 4: 通ることを確かめる** — Run: `cargo test -p routelog-geo` / Expected: PASS
- [ ] **Step 5: コミット**

---

### Task 5: タイルの取得と保存、抽出への組み込み

**Files:**
- Modify: `crates/routelog-geo/src/official.rs`（`Store`）、`crates/routelog-geo/Cargo.toml`（`ureq = "2"`、`serde_json`）
- Modify: `app/src-tauri/src/events.rs`、`app/src-tauri/src/lib.rs`、`app/src-tauri/src/rides.rs`（コマンドの登録と抽出）
- Create: `crates/routelog-geo/examples/ride_structures.rs`

**Interfaces:**
- Consumes: タスク 3・4
- Produces:
  - `pub struct Store`、`Store::new(dir: impl Into<PathBuf>) -> Store`：`dir/tunnel/<x>-<y>.pbf` と `dir/bridge/<x>-<y>.pbf` に置く。施設の無いタイルは空のファイルで覚える
  - `Store::missing(&self, tiles: &BTreeSet<(u32, u32)>) -> usize`、`Store::fetch(&self, tiles: &BTreeSet<(u32, u32)>) -> Result<usize, Error>`（足りないものだけ取り、取った数を返す。TileJSON は 1 回の起動で 1 度読む。間隔は 50 ms 以上）
  - `Store::facilities(&self, tiles: &BTreeSet<(u32, u32)>) -> Vec<Facility>`：手元にあるタイルだけを読む。通信しない
  - Tauri のコマンド `fetch_structures(doc: RideDocument) -> Result<usize, String>`：その走行の経路のタイルを取り、取った数を返す。置き場所はアプリのキャッシュのフォルダの `qchizu`（自動の確認では `check::data_dir()` の下。overview.rs の `tiles` と同じ決め方）
  - `Extractor::extract` は、`Tiles::extract` のあとに `Store::facilities` と `official::attach` を当ててから `carry_over` する
  - 例：`cargo run --release -p routelog-geo --example ride_structures -- <判定用データのフォルダ> <走行の JSON> <タイルのフォルダ>`。足りないタイルを取り、橋・トンネルを「km shown_name（字幕なしなら印）」で出す

- [ ] **Step 1: 失敗するテストを書く**（official.rs）：`Store` に一時フォルダを渡し、空のファイルを置いたタイルは `missing` に数えず、`facilities` は空を返す（`an_empty_file_means_no_facilities`）
- [ ] **Step 2: 失敗を確かめる** — Run: `cargo test -p routelog-geo store` / Expected: FAIL
- [ ] **Step 3: 実装する**（ureq は routelog-overview の `TileStore` と同じ作り）
- [ ] **Step 4: 通ることを確かめる** — Run: `cargo test --workspace` と `cargo clippy --workspace --all-targets` / Expected: PASS、警告なし
- [ ] **Step 5: 受け入れ条件を実データで確かめる** — Run: 例を 2026-10-03 の走行で（タイルのフォルダはスクラッチパッド。走行は読むだけ）/ Expected: design.md の受け入れ条件の 5 つ
- [ ] **Step 6: コミット**

---

### Task 6: 画面（名前の出し方、候補を選ぶ、人の切り替えの記録、タイルの取得）

**Files:**
- Modify: `app/src/lib/types.ts`、`app/src/lib/events.ts`（`shownName`、`sameName`、`setHidden`、`setName`）、`app/src/lib/events.test.ts`
- Modify: `app/src/lib/EventName.svelte`、`app/src/lib/EventList.svelte`、`app/src/App.svelte`

**Interfaces:**
- Consumes: タスク 1 の項目と名前の決まり、タスク 5 の `fetch_structures`
- Produces:
  - `RideEvent.detail` に `official_name?`、`official_length_m?`、`hidden_by_hand?`、`named_by_hand?`
  - `shownName(event: RideEvent): string`、`sameName(a: string, b: string): boolean`：Rust と同じ決まり
  - `nameChoices(event: RideEvent): string[]`：名前が違えば `[label, official_name]`、同じか無ければ空
  - `setHidden` は `detail.hidden_by_hand = true` も付ける。`setName(event, label)` は `label` と `detail.named_by_hand = true` を付ける
  - 一覧の行を開いた所の名前の欄の下に、`nameChoices` を押せる小さなボタンで並べる。押すと `setName`
  - `EventName` は、橋・トンネルで `shownName` を出す
  - App：走行を開いたとき（`switchTo` のあと）と、経路を保存して抽出したあとに、`fetch_structures` を裏で呼ぶ。取った数が 1 以上なら `extractAgain()`。失敗は知らせない（通信できないだけ）

- [ ] **Step 1: 失敗するテストを書く**（events.test.ts）：タスク 1 のテストと同じ例で `shownName` と `sameName`、`setHidden` と `setName` が印を付けること、`nameChoices`
- [ ] **Step 2: 失敗を確かめる** — Run: `app/` で `mise exec -- bun test events` / Expected: FAIL
- [ ] **Step 3: 実装する**
- [ ] **Step 4: 通ることを確かめる** — Run: `mise exec -- bun test`、`mise exec -- bunx svelte-check --threshold warning` / Expected: PASS、警告なし
- [ ] **Step 5: 画面を確かめる**：地図の確認（`ROUTELOG_CHECK_MAP=-`）を走らせ、壊れていないこと
- [ ] **Step 6: コミット**

---

### Task 7: 設計書とリリースビルド

- [ ] **Step 1**：design.md の節の見出しを「2026-10-25 に実装」にし、作り（モジュール、置き場所、点数の決め方、閾値）と、実データの結果を書く。決定事項に 1 行足す。「地図・道路データ」の表と「判定用データ」の節の、字幕なしの長さ（50 m）の記述を直す
- [ ] **Step 2**：`cargo test --workspace`、`cargo clippy --workspace --all-targets`、`cargo fmt --check`、`bun test` / Expected: PASS
- [ ] **Step 3**：`app/` で `mise exec -- bun run tauri build` / Expected: `target/release/routelog.exe`
- [ ] **Step 4**：コミット
