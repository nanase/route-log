# データ

走行（Ride）を単位に、経由地・アンカー・イベントを持ちます。経路の線は経由地から再計算できますが、手修正を残すため確定したジオメトリも保存します。

| エンティティ | 保存先 | 主な項目 |
| --- | --- | --- |
| Ride（走行） | D1 | id, title, date, clock_offset_ms, is_public, labels, episodes, created_at, updated_at |
| Waypoint（経由地） | D1 | ride_id, seq, id, lon, lat, segment_mode（straight / snap / manual） |
| Episode（パート） | D1（rides.episodes） | id, title, from, to（端の経由地の ID）。[パート](ui.md#パート) にある |
| Route（経路） | R2 | 経由地から生成した LineString（GeoJSON）、手修正後の頂点を含む |
| Anchor（時刻確定点） | D1 | ride_id, route_m（経路上の距離 m）, time_ms, source（manual / video / gps）。まだ作っていない。いまは時刻を確定したイベントが基準の役をする |
| Event（字幕イベント） | D1 | ride_id, kind, label, route_m, time_ms, status（estimated / confirmed）, hidden（字幕なし）, detail_json |
| VideoFile（素材） | ローカルのみ | path, start_ms, duration_ms, codec |

## Event.kind

- `departure` / `arrival`：出発・到着（場所名は手入力）。経路の両端に 1 つずつ、自動で置く
- `pref_boundary` / `muni_boundary`：都道府県・市区町村の境（label 例：「長野県 → 群馬県」）。政令市の区の境も `muni_boundary` にする
- `road`：走行路線の切り替わり。重用区間を含む全路線の種別（国道／都道府県道）と番号を detail_json に持ち、標識画像を選ぶ手がかりにする
- `bridge` / `tunnel`：橋・トンネル（開始位置。終了位置は detail_json）。対応づけた施設の公称の名前と長さ（`official_name`・`official_length_m`）も detail_json に持つ
- `michi_no_eki`：道の駅（経路から一定距離以内を通過した位置）
- `waypoint`：経由地。そこを通った時刻を入れたいと人が選んだ経由地にだけ作る。字幕なしで作り、時刻推定の基準に使う
- `stop`：停車。始まりの時刻と、停車していた長さ（`stay_ms`）を持つ。その間は位置が変わらないものとして時刻を推定する。位置は、地図で決めてあればその場所（`detail.at`）、無ければ時刻から求める。字幕なしで作る
- `custom`：手動で追加したイベント。動画のいまの位置に足したものは時刻が分かっていて位置が分からないので、位置を時刻から求め、時刻推定の基準にはしない（`by_time`）

## 時刻の状態と、字幕にするかどうか

- `status` は時刻のことだけを表す。推定（estimated）か、人が確定した（confirmed）かである
- 字幕にするかどうかは、別に `hidden` で持つ。字幕なしにしても、確定した時刻は残り、時刻推定の基準になる
  - 当初は `status` に hidden を持っていた。字幕なしにすると確定した時刻が消え、「字幕には出さないが、時刻は入れたい」地点を作れなかった
  - 以前の形で保存した走行は、読むときにいまの形へ直す
- 人が字幕の有無を切り替えたら `hidden_by_hand`、名前を直すか候補から選んだら `named_by_hand` を detail_json に付ける。抽出し直しても、橋・トンネルの字幕の有無は、`hidden_by_hand` の付いたものだけを受け継ぎ、ほかは決め直す

## 経路の持ち方

- 経路は、経由地の列と、経由地の間の頂点でできている
  - `route.segments[i]` が、経由地 `i` と `i + 1` の間の頂点を持つ。直線なら空
  - 区間の数は、いつも経由地の数より 1 つ少ない
- 頂点を足した区間は `manual` になり、頂点が無くなれば `straight` に戻る
- 途中の経由地を消すと前後の区間が 1 つになり、手で置いた頂点は残る
- LineString は、経由地と頂点を順に並べれば得られる

## 経路上の位置

位置は緯度経度ではなく、経路の始点からの距離 `route_m` で持ちます。経路を手修正したときは、イベントを再抽出してから、確定済みのものを近い位置で引き継ぎます。

## D1 のスキーマ

正は `worker/migrations/` の SQL です。いまの表は次のとおりです。アンカーの表はまだ作っていません。

| 表 | 列 | 足したマイグレーション |
| --- | --- | --- |
| `rides` | `id`（UUIDv7）、`title`、`ride_date`（JST の YYYY-MM-DD）、`clock_offset_ms`、`is_public`、`route_key`（R2 のキー）、`created_at`、`updated_at` | 0001 |
| | `labels`（ラベルの並び、JSON） | 0002 |
| | `episodes`（パートの並び、JSON） | 0003 |
| `waypoints` | `ride_id`、`seq`、`lon`、`lat`、`segment_mode`（次の経由地までの結び方） | 0001 |
| | `id`（UUIDv7。無い頃の行は NULL） | 0003 |
| `events` | `id`、`ride_id`、`seq`、`kind`、`label`、`route_m`、`time_ms`、`status`、`json`（イベントの全体） | 0001 |

- `waypoints` と `events` は、`rides` を消すと一緒に消える（`ON DELETE CASCADE`）
- `events` は、絞り込みに使う値だけを列に出し、全体を `json` に持つ。Worker は中身を見ずに往復させる（[Worker の実装](cloud.md#worker-の実装)）
- 経路の線は D1 に置かず、R2 の `rides/<ID>/route.json` に置く
