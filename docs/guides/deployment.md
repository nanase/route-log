# 設定とデプロイ

走行ログの API を Cloudflare に置き、アプリから同期できるようにする手順です。手順 1〜10 は、初めて作るときに 1 度だけ行います。作ったあとの更新は [マイグレーション](#マイグレーション) と [Worker を入れ替える](#worker-を入れ替える) にあります。

作るものは 5 つあります。

| 作るもの | 何に使うか |
| --- | --- |
| D1 のデータベース `routelog` | 走行・経由点・イベント |
| R2 のバケット `routelog-routes` | 経路の線 |
| Worker `routelog` | API |
| Access のサービストークン | アプリが API を呼ぶための資格情報 |
| Access のアプリケーション | API の前に立ち、サービストークンの無い要求を締め出す |

コマンドはどれも `worker/` で実行します。`bun wrangler` の前には `mise exec --` を付けます。

## 始める前に決めること

- API のホスト名
  - Cloudflare で管理しているドメインの下に置く。この文書では `routelog.example.com` と書く
- Access のチームのドメイン
  - `<チーム名>.cloudflareaccess.com` の形。Zero Trust の設定画面に出ている

## 1. ログインする

```sh
mise exec -- bun wrangler login
mise exec -- bun wrangler whoami
```

ブラウザが開きます。`whoami` でアカウントが出れば済んでいます。

## 2. D1 と R2 を作る

```sh
mise exec -- bun wrangler d1 create routelog
mise exec -- bun wrangler r2 bucket create routelog-routes
```

`d1 create` が `database_id` を出します。`wrangler.toml` の `database_id` を、その値に書き換えます。この値はコミットして構いません。資格情報が無ければ、ID だけではデータベースに届きません。

## 3. スキーマを当てる

```sh
mise exec -- bun wrangler d1 migrations apply routelog --remote
```

## 4. `wrangler.toml` を埋める

- `ACCESS_TEAM_DOMAIN` に、チームのドメインを入れる
- 末尾の `[[routes]]` の `pattern` を、API のホスト名にする

```toml
[vars]
ACCESS_TEAM_DOMAIN = "<チーム名>.cloudflareaccess.com"

[[routes]]
pattern = "routelog.example.com"
custom_domain = true
```

## 5. デプロイする

```sh
mise exec -- bun wrangler deploy
```

この時点では、API はすべての要求を 403 で拒みます。`ACCESS_AUD` がまだ無いためです。Access を作る前にデプロイしても、中身が外へ出ることはありません。

## 6. サービストークンを作る

1. ダッシュボードで **Zero Trust** > **Access controls** > **Service credentials** > **Service Tokens** を開く
2. **Create Service Token** を押し、名前と有効期間を決めて **Generate token** を押す
3. 出てきた Client ID と Client Secret を控える

Client Secret が出るのはこの 1 度だけです。控えそこねたら、作り直すことになります。

## 7. Access のアプリケーションを作る

1. **Zero Trust** > **Access controls** > **Applications** で **Create new application** を押す
2. **Self-hosted and private** を選び、**Add public hostname** でホスト名を入れる
3. ポリシーを 1 つ足す
   - Action を **Service Auth** にする
   - 条件に、手順 6 で作ったサービストークンを選ぶ
4. **Create** を押す

Action が Service Auth でないと、Access はログイン画面へ送ろうとし、アプリからは通れません。

## 8. `ACCESS_AUD` を置く

作ったアプリケーションの設定画面に、Application Audience（AUD）タグがあります。それをシークレットとして置きます。

```sh
mise exec -- bun wrangler secret put ACCESS_AUD
```

値を尋ねられるので、貼り付けます。コマンドの引数にはしないでください。シェルの履歴に残ります。

## 9. 確かめる

まず、ヘッダを付けない要求が通らないことを確かめます。Access のエラー画面（403）が返れば、Access が前に立っています。

```sh
curl -i https://routelog.example.com/api/rides
```

サービストークンを付けた要求は、手順 10 のアプリで確かめます。curl のコマンドに Client Secret を書くと、シェルの履歴に残るためです。

どうしても curl で確かめるなら、値を画面にも履歴にも出さずに読み込みます。

```sh
read -rs CLIENT_ID      # Client ID を貼って Enter。画面には出ない
read -rs CLIENT_SECRET  # Client Secret を貼って Enter
curl -H "CF-Access-Client-Id: $CLIENT_ID" -H "CF-Access-Client-Secret: $CLIENT_SECRET" https://routelog.example.com/api/rides
unset CLIENT_ID CLIENT_SECRET
```

`[]` が返れば通っています。

## 10. アプリに設定する

1. アプリの上の帯の右端にある「設定」を押す
2. 「クラウド同期の接続先」の「設定する」を押す
3. API の URL、Client ID、Client Secret を入れて「接続先を保存」を押す
4. 設定を閉じ、上の帯の右側の表示を見る
5. 「同期中…」のあと「同期済み」と出れば繋がっている。手元の走行は、自動でクラウドへ送られる

「未同期」のままなら、繋がっていません。「未同期」にマウスを重ねると、理由が出ます。

Client ID と Client Secret は、Windows の資格情報マネージャーに保存されます。設定ファイルには書かれません。

## うまくいかないとき

Worker のログを見ます。トークンを拒んだときは、理由が 1 行出ます。

```sh
mise exec -- bun wrangler tail
```

| ログの理由 | 見る所 |
| --- | --- |
| 発行者が違います | `ACCESS_TEAM_DOMAIN` |
| 宛先が違います | `ACCESS_AUD` |
| 持ち主が分かりません | サービストークンのトークンに `common_name` が入っているか。下の「まだ確かめていないこと」を参照 |
| ログに何も出ない | 要求が Worker に届いていない。Access のポリシーか、ホスト名を見る |

## 戻し方

```sh
mise exec -- bun wrangler rollback        # 1 つ前のデプロイに戻す
mise exec -- bun wrangler delete          # Worker を消す
```

D1 と R2 は、ダッシュボードか `wrangler d1 delete` / `wrangler r2 bucket delete` で消します。Access のアプリケーションとサービストークンは、ダッシュボードで消します。

## マイグレーション

D1 のスキーマは `worker/migrations/` の連番の SQL で変えます。

- 変更 1 つにつき、`worker/migrations/` に次の番号の SQL を足す
- 同じ名前の戻し用の SQL を `worker/migrations/rollback/` に置く
- 当てたファイルは書き換えない
  - wrangler は当てたファイルを名前で覚えるので、書き換えた中身は当て済みのデータベースに届かない
- 手元の D1 で当ててから、本番に当てる（[Worker の確認](development.md#worker-の確認)）

```sh
mise exec -- bun run migrate:local                                  # 手元
mise exec -- bun wrangler d1 migrations apply routelog --remote     # 本番
```

## Worker を入れ替える

新しいマイグレーションがあれば、Worker より先に当てます。新しい Worker は、足した列を読むためです。古い Worker は、足した列を知らずに捨てます。

```sh
mise exec -- bun wrangler d1 migrations list routelog --remote   # 当てていないものを見る
mise exec -- bun wrangler d1 time-travel info routelog           # 戻す先の bookmark を控える
mise exec -- bun wrangler d1 migrations apply routelog --remote
mise exec -- bun wrangler deploy
curl -s -o /dev/null -w "%{http_code}\n" https://routelog.example.com/api/rides   # 403 なら Access が前に立っている
```

マイグレーションを戻すときは、`wrangler d1 time-travel restore routelog --bookmark=<控えた bookmark>` で、当てる前の状態にします。`migrations/rollback/` に戻し用の SQL もあります。

## 確かめたことと、まだのこと

2026-10-13 に、この手順で `routelog.nanase.cc` を作りました。

- 確かめたこと
  - `ACCESS_AUD` を置く前は、Worker がすべての要求を 403 で拒む
  - Access のアプリケーションを作ると、ヘッダの無い要求は Access のエラー画面（403）で止まる
  - アプリから走行を 1 つ送ると、状態が「同じ」になる。本番の D1 と R2 に書けている
  - アプリにサービストークンを入れると、一覧を取れる。サービストークンのトークンは、持ち主を `common_name` に持つ
- 初回のデプロイで出たエラー
  - `This Worker does not exist on your account. [code: 10007]` が出た。アップロードは済んでいて、もう一度 `wrangler deploy` を実行すると通った
- まだのこと
  - 手順 7・8 の画面の細かい名前。ポリシーの条件の欄の名前と、AUD タグの置き場所は、Cloudflare の文書では確かめられなかった
  - GitHub Actions からのデプロイ。いまは手元から `wrangler deploy` する
