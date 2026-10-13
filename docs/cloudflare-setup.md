# Cloudflare 側の準備

走行ログの API を Cloudflare に置き、アプリから同期できるようにする手順です。作るものは 5 つあります。

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
- 末尾の `[[routes]]` の 3 行のコメントを外し、`pattern` をホスト名にする

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

```sh
curl -H "CF-Access-Client-Id: <Client ID>" -H "CF-Access-Client-Secret: <Client Secret>" https://routelog.example.com/api/rides
```

`[]` が返れば通っています。ヘッダを付けない要求が通らないことも確かめてください。

## 10. アプリに設定する

1. アプリの上の帯の「同期」を押す
2. API の URL、Client ID、Client Secret を入れて「設定を保存」を押す
3. 「一覧を取り直す」で、手元の走行が「手元だけ」と出れば繋がっている

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

## まだ確かめていないこと

手元の Worker では、その場で作った鍵で署名したトークンを使って API を確かめました。本物の Access を通した確認は、この手順が初めてになります。

- サービストークンのトークンが、持ち主を `common_name` に持つこと
  - 違っていたら、`routelog-access` の持ち主の読み方を直す
- 手順 7・8 の画面の細かい名前
  - サービストークンの画面と、アプリケーションを作り始める所までは、Cloudflare の文書で確かめた
  - ポリシーの条件の欄の名前と、AUD タグの置き場所は、文書では確かめられなかった
- GitHub Actions からのデプロイ
  - まだ組んでいない。いまは手元から `wrangler deploy` する
