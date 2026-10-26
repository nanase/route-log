# 判定用データ

イベントの抽出と吸着に使う判定用データを、国道マップの生成物から書き出す手順です。書き出したものは `data/geo/` に置かれ、git の管理外です。中身と作り方の理由は [地図・道路データ](../reference/geo.md) にあります。

## 始める前に

国道マップ（[nanase/kokudo-map](https://github.com/nanase/kokudo-map)）をクローンし、ビルドを済ませておきます。書き出しが読むのは、そのフォルダの `build/` の下にある次のものです。

| 場所 | 中身 |
| --- | --- |
| `build/regions/*.geojson` | 地域ごとの国道 |
| `build/prefectural/*.geojson` | 地域ごとの都道府県道 |
| `build/pbf/japan-latest.osm.pbf` | OSM の日本全域のデータ |
| `build/n03/2026/*.zip` | 国土数値情報 N03（行政区域） |

国道マップのフォルダには何も書きません。

## フォルダを渡す

国道マップのフォルダは、環境変数 `KOKUDO_MAP_DIR` で渡します。場所はマシンごとに違うので、git の管理外の `mise.local.toml` に書きます。

```toml
[env]
KOKUDO_MAP_DIR = "<国道マップのフォルダ>"
```

## 書き出す

ルートで実行します。uv が Python と依存を用意します。

```sh
mise run export-geo
```

全部を作り直すと、10 分ほどかかります。一部だけ作り直すときは、`--only` に `roads`・`eki`・`admin` から選んで渡します。

```sh
mise exec -- uv run pipeline/export_geo.py --only admin
```

| ファイル | 中身 | 大きさ | 時間 |
| --- | --- | --- | --- |
| `roads.fgb` | 国道・都道府県道と、それ以外の道 | 約 1 GB | 約 470 秒 |
| `michi-no-eki.fgb` | 道の駅の位置と名前 | 0.2 MB | 約 90 秒 |
| `admin.fgb` | 市区町村（政令市は区）の面 | 49 MB | 約 75 秒 |

## アプリで使う

アプリの設定の「判定用データ」で `data/geo/` を選びます。選ぶと、経路を保存するたびにイベントを抽出し、地図で「道に沿わせる」を選べるようになります。

## 国道マップが変わったとき

書き出しは、国道マップの生成物の形に依っています。国道マップの側で形が変わって書き出しが失敗したら、`pipeline/export_geo.py` を直します。国道マップの側には手を入れません。
