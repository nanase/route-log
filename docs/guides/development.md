# 開発

手元でアプリと Worker を動かし、検査するときの手順です。動作は Windows で確かめています。

## 準備

道具の版は `mise.toml` で固定しています。[mise](https://mise.jdx.dev/) を使えば、node・bun・ffmpeg・python・uv の固定した版が入ります。

```sh
mise install
```

PATH に別の版の node や ffmpeg があっても、`mise exec --` を付けたコマンドは固定した版で動きます。この文書のコマンドにはすべて付けています。

Rust は [rustup](https://rustup.rs/) で入れ、Worker 用の target を足します。

```sh
rustup target add wasm32-unknown-unknown
cargo install worker-build
```

依存は `app/` と `worker/` で別々に入れます。

```sh
cd app && mise exec -- bun install --frozen-lockfile
cd worker && mise exec -- bun install --frozen-lockfile
```

動画の再生には、Windows の「HEVC ビデオ拡張機能」が要ります（[動画の再生](../reference/playback.md)）。

## アプリの起動

`app/` で起動します。vite が 1420 番で画面を配信し、Tauri が窓を開きます。

```sh
cd app
mise exec -- bun run tauri dev
```

`mise exec --` を付けずに起動すると、PATH に ffprobe が無く、動画を読めません（[ffprobe の探し方](../reference/timeline.md#ffprobe-の探し方)）。

起動したら、設定で次のフォルダを選びます。どちらも無くても、経路の記録はできます。

| 設定 | 中身 | 用意のしかた |
| --- | --- | --- |
| 道路の線 | 国道マップの配信データ（PMTiles） | 国道マップのビルドで作る |
| 判定用データ | `data/geo/` | [判定用データ](geo-data.md) |

開発中のアプリも、リリースビルドと同じデータフォルダを使います。本物の走行に触れずに試すときは、環境変数 `ROUTELOG_DATA_DIR` で別のフォルダを渡します（[ローカルの保存](../reference/architecture.md#ローカルの保存)）。

## テストと型検査

Rust のテストは、ルートで実行します。Worker は wasm 向けなので、`default-members` から外してあり、ここには入りません。

```sh
cargo test
cargo clippy
```

画面のテストと型検査は `app/` で実行します。

```sh
cd app
mise exec -- bun test
mise exec -- bun run check
```

## 動作確認

アプリには、人が画面を見なくても動作を確かめる仕組みがあります。環境変数を与えて起動すると確認が自動で走り、報告を書いて終了します。入口は `app/src-tauri/src/check.rs` にあります。

| 環境変数 | 確かめること |
| --- | --- |
| `ROUTELOG_CHECK_FILE` | 動画 1 本の読み込み・再生・シーク |
| `ROUTELOG_CHECK_DIR` | フォルダの中のいちばん古い日付で走行を作り、そのフォルダを動画フォルダに足す。ファイルをまたぐ再生と時刻の変換、動画を走行から外す操作、停車の候補を探して外す操作 |
| `ROUTELOG_CHECK_MAP` | 地図の表示、経路の編集・保存、概略図の動画の見本と書き出し、走行のラベルの付け方と絞り込み。値は道路データのフォルダで、重ねないなら `-` |
| `ROUTELOG_CHECK_GEO` | 値は判定用データのフォルダ。下の表のとおり、ほかの確認に足して使う |
| `ROUTELOG_CHECK_SYNC` | クラウドとの自動の同期。値は手元で動かした Worker の URL（[同期の確認](#同期の確認)） |
| `ROUTELOG_CHECK_STEP` | 同期の確認の段。ほかの確認に足すと、確かめることが変わる |
| `ROUTELOG_CHECK_REPORT` | 報告（JSON）の書き先。画面の写し（PNG）も同じ場所に書く |

ほかの確認に足すと、次のように変わります。

| 組み合わせ | 確かめること |
| --- | --- |
| `ROUTELOG_CHECK_MAP` と `ROUTELOG_CHECK_GEO` | イベントの抽出と吸着。待たずに続けてクリックしても、経由地が押した順に並ぶこと |
| `ROUTELOG_CHECK_DIR` と `ROUTELOG_CHECK_GEO` | 動画を見ながらの時刻の確定と、動画の無い走行で時刻を手で入れること |
| `ROUTELOG_CHECK_MAP` と `ROUTELOG_CHECK_STEP=close` | 編集した直後に窓を閉じる。保存を待っていた編集が書かれたかは、走行のファイルを見て確かめる。報告は書かない |
| `ROUTELOG_CHECK_DIR` と `ROUTELOG_CHECK_STEP=episodes` | パートの作り方・直し方・消し方、録画の長さ、Resolve への書き出し。録画のいちばん長い日付の動画を使い、書き出しは報告と同じ場所に書く |

- 確認の間は、走行を報告の隣のフォルダに書く。本物のデータには触らない
  - OS の資格情報ストアにあるサービストークンも読まない
  - 地図の表示位置などブラウザ側の保存も分けるなら、`WEBVIEW2_USER_DATA_FOLDER` を一時フォルダに向ける
- 停車の候補を探す確認は、フォルダの動画をすべて測る。数分の動画を切り出したフォルダで行う
  - 停車を含む 2〜3 分ずつを 2 本切り出したフォルダでは、290 秒ぶんを 45 秒で探せた
- 地図の操作は、実際のマウスイベントを送って確かめる
- 写るのはアプリの画面だけである

```sh
cd app
ROUTELOG_CHECK_MAP=- ROUTELOG_CHECK_REPORT=<一時フォルダ>/report.json mise exec -- bun run tauri dev
```

## Worker の確認

`worker/` で次を行うと、Cloudflare に触らずに API を一通り確かめられます。

1. `.dev.vars` に次の 3 行を書く。値は `test/local.mjs` の先頭のコメントにある

   ```sh
   ACCESS_TEAM_DOMAIN=test.example
   ACCESS_AUD=test-aud
   ACCESS_CERTS_URL=http://127.0.0.1:8799/certs
   ```

2. 手元の D1 にスキーマを当てる

   ```sh
   mise exec -- bun run migrate:local
   ```

3. 確認用のスクリプトを起動し、鍵を配り始めたら Worker を起動する

   ```sh
   node test/local.mjs
   mise exec -- bun run dev    # 別の端末で
   ```

- 確認用のスクリプトは、その場で作った鍵でトークンに署名し、公開鍵を配る。本物の Access には触らない
- `ACCESS_CERTS_URL` は、鍵を取る場所を差し替えるための設定である。本番の設定には置かない
- `.dev.vars` は git の管理外である
- wasm 用の道具は、初回のビルドで自動で取ってくる

## 同期の確認

アプリからの同期は、2 つの端末の間で確かめます。端末は、データフォルダ（`ROUTELOG_DATA_DIR`）を変えて起動することで真似ます。

1. 上の手順で Worker を動かす。`node test/local.mjs` は `--token` を付けて起動し、出たトークンを控える
2. `ROUTELOG_CHECK_SYNC` に Worker の URL を、`ROUTELOG_SYNC_JWT` にトークンを渡す
3. 下の表の段を、上から順に 1 回の起動で 1 つずつ行う。段は `ROUTELOG_CHECK_STEP` で選ぶ

名前の頭の a と b が端末です。同じ端末には同じデータフォルダを渡します。

| 段 | 確かめること |
| --- | --- |
| `a-create` | 走行を作って編集すると、押さなくても送られる。続けて編集している間は送らない。通信できないと「未同期」になり、編集と保存は続けられる。繋がると、60 秒後か次の保存で送られる |
| `b-receive` | 別の端末で起動すると、クラウドの走行を自動で受け取る |
| `a-update` | 別の端末で直した内容を受け取り、開いている走行を読み直す |
| `b-diverge` | 端末 A が送れずにいる間に、同じ走行を直して送る |
| `a-conflict` | 送ろうとして 412 が返ると、上書きせずに「両方で変更」になる。上の帯から板を開き、手元を選んで揃える |
| `b-conflict` | 保存を待っている編集があるときに受け取りが来ても、上書きしない。クラウドを選んで揃えると、開いている走行が変わる |
| `a-diverge` | 端末 B が送れずにいる間に、同じ走行を直して送る |
| `b-startup-conflict` | 起動のときの突き合わせで、両方で変わっていると分かる |
| `a-delete` | 走行を消すと、クラウドからも消える。通信できないときは、消さずに理由を出す。1 度も送っていない走行は消せる |
| `b-after-delete` | 端末 A が消した走行が、端末 B から送り直されて戻ることを記録する。最後に、確認で作った走行を消す |

- 通信できない状態は、接続先を、誰も聞いていないポートに差し替えて作る
- 上の帯の表示は、画面に出ている字を読んで確かめる
- `ROUTELOG_SYNC_JWT` は手元の Worker のためのもので、Access を通す本番では意味を持たない

同期の決まりは [クラウド API と同期](../reference/cloud.md) にあります。

## リリースビルド

`app/` で実行します。インストーラーは作らず、`target/release/routelog.exe` だけができます。

```sh
cd app
mise exec -- bun run tauri build
```

できた exe は、判定用データ・道路データ・ffmpeg を含みません。開発中と同じ場所から読みます。ffmpeg の場所は、リポジトリの中で 1 度起動したときに覚えるので、exe を別の場所に写す前に 1 度起動しておきます（[リリースビルド](../reference/architecture.md#リリースビルド)）。
