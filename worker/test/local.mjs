// 手元で動かした Worker（`bun run dev`）に対して、API を一通り確かめる。
//
// Access の代わりに、この場で作った鍵でトークンに署名し、その公開鍵を配る。Worker は
// `.dev.vars` の ACCESS_CERTS_URL から鍵を取る。本物の Access には触らない。
//
// 使い方（worker/ で）:
//   1. .dev.vars に次を書く
//        ACCESS_TEAM_DOMAIN=test.example
//        ACCESS_AUD=test-aud
//        ACCESS_CERTS_URL=http://127.0.0.1:8799/certs
//   2. bun run migrate:local
//   3. node test/local.mjs を先に起動し、鍵を配り始めたら bun run dev を起動する
//
// アプリからの同期を確かめるときは、node test/local.mjs --token とする。1 時間使える
// トークンを 1 行出し、止められるまで鍵を配り続ける。

import { createSign, generateKeyPairSync } from "node:crypto";
import { createServer } from "node:http";

const API = process.env.ROUTELOG_API ?? "http://127.0.0.1:8787";
const TEAM = "test.example";
const AUD = "test-aud";
const ID = "0199c0de-0000-7000-8000-00000000000a";

const { publicKey, privateKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
const jwk = { ...publicKey.export({ format: "jwk" }), kid: "local-key" };
const certs = createServer((_, res) => res.end(JSON.stringify({ keys: [jwk] }))).listen(8799, "127.0.0.1");

const encode = (value) => Buffer.from(JSON.stringify(value)).toString("base64url");
function token(claims = {}) {
  const now = Math.floor(Date.now() / 1000);
  const body = { iss: `https://${TEAM}`, aud: [AUD], exp: now + 600, common_name: "local.access", ...claims };
  const signed = `${encode({ alg: "RS256", kid: "local-key" })}.${encode(body)}`;
  return `${signed}.${createSign("RSA-SHA256").update(signed).sign(privateKey).toString("base64url")}`;
}

async function call(method, path, { jwt = token(), body, ifMatch } = {}) {
  const headers = { "Content-Type": "application/json" };
  if (jwt !== null) headers["Cf-Access-Jwt-Assertion"] = jwt;
  if (ifMatch !== undefined) headers["If-Match"] = ifMatch;
  const response = await fetch(API + path, { method, headers, body: body && JSON.stringify(body) });
  const text = await response.text();
  return { status: response.status, body: text === "" ? null : JSON.parse(text) };
}

const document = (updated_at, title = "碓氷峠") => ({
  ride: { id: ID, title, ride_date: "2026-10-03", clock_offset_ms: -30000, is_public: false, created_at: "t0", updated_at },
  waypoints: [
    { lon: 138.62, lat: 36.345, segment_mode: "manual" },
    { lon: 138.7, lat: 36.36, segment_mode: "straight" },
  ],
  route: { segments: [[[138.66, 36.35]]] },
  events: [
    { id: "0199c0de-0000-7000-8000-0000000000e1", kind: "departure", label: "出発", route_m: 0, time_ms: 1790000000000, status: "confirmed", detail: { camera_ms: 1790000030000 } },
    { id: "0199c0de-0000-7000-8000-0000000000e2", kind: "road", label: "国道18号", route_m: 120.5, time_ms: null, status: "estimated", detail: { routes: [{ class: "national", number: 18 }], extracted_label: "国道18号" } },
  ],
});

if (process.argv.includes("--token")) {
  console.log(token({ exp: Math.floor(Date.now() / 1000) + 3600 }));
  await new Promise(() => {});
}

let failed = 0;
function check(name, actual, expected) {
  const same = JSON.stringify(actual) === JSON.stringify(expected);
  if (!same) failed += 1;
  console.log(`${same ? "ok  " : "NG  "} ${name}${same ? "" : `\n      期待 ${JSON.stringify(expected)}\n      実際 ${JSON.stringify(actual)}`}`);
}

async function waitForWorker() {
  for (let i = 0; i < 120; i++) {
    try {
      await fetch(API + "/api/rides");
      return;
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 1000));
    }
  }
  throw new Error(`${API} に繋がりません`);
}

try {
  await waitForWorker();
  await call("DELETE", `/api/rides/${ID}`);

  check("トークンが無ければ 403", (await call("GET", "/api/rides", { jwt: null })).status, 403);
  check("別の宛先のトークンは 403", (await call("GET", "/api/rides", { jwt: token({ aud: ["other"] }) })).status, 403);
  check("期限の切れたトークンは 403", (await call("GET", "/api/rides", { jwt: token({ exp: 1000 }) })).status, 403);
  check("署名の合わないトークンは 403", (await call("GET", "/api/rides", { jwt: token().slice(0, -8) + "AAAAAAAA" })).status, 403);
  check("api の外は 404", (await call("GET", "/other")).status, 404);
  check("UUID の形でない ID は 404", (await call("GET", "/api/rides/../secret")).status, 404);

  check("無い走行は 404", (await call("GET", `/api/rides/${ID}`)).status, 404);
  const placed = await call("PUT", `/api/rides/${ID}`, { body: document("t1") });
  check("新しい走行を置ける", [placed.status, placed.body.updated_at], [200, "t1"]);
  check("置いたものがそのまま返る", (await call("GET", `/api/rides/${ID}`)).body, document("t1"));
  check("一覧に出る", (await call("GET", "/api/rides")).body.filter((r) => r.id === ID), [{ id: ID, title: "碓氷峠", ride_date: "2026-10-03", updated_at: "t1" }]);

  check("If-Match が無ければ上書きできない", (await call("PUT", `/api/rides/${ID}`, { body: document("t2") })).status, 412);
  const stale = await call("PUT", `/api/rides/${ID}`, { body: document("t2"), ifMatch: "t0" });
  check("古い更新時刻では上書きできず、いまの更新時刻が返る", [stale.status, stale.body.updated_at], [412, "t1"]);
  check("知っている更新時刻なら上書きできる", (await call("PUT", `/api/rides/${ID}`, { body: document("t2", "碓氷峠（修正）"), ifMatch: "t1" })).status, 200);
  check("上書きしたものが返る", (await call("GET", `/api/rides/${ID}`)).body, document("t2", "碓氷峠（修正）"));

  const shorter = { ...document("t3"), waypoints: [], route: { segments: [] }, events: [] };
  check("経由点とイベントを空にできる", (await call("PUT", `/api/rides/${ID}`, { body: shorter, ifMatch: "t2" })).status, 200);
  check("空にしたものが返る", (await call("GET", `/api/rides/${ID}`)).body, shorter);

  // 以前の形（字幕にしないことを状態で持つ）で置いても、いまの形で返る。
  const hiddenEvent = (extra) => ({ id: "0199c0de-0000-7000-8000-0000000000e3", kind: "bridge", label: "小さな橋", route_m: 300, time_ms: null, ...extra, detail: {} });
  const old = { ...shorter, ride: { ...shorter.ride, updated_at: "t3a" }, events: [hiddenEvent({ status: "hidden" })] };
  check("以前の形のイベントを置ける", (await call("PUT", `/api/rides/${ID}`, { body: old, ifMatch: "t3" })).status, 200);
  check("字幕にしないことが、状態とは別に返る", (await call("GET", `/api/rides/${ID}`)).body.events, [hiddenEvent({ status: "estimated", hidden: true })]);
  const timed = { ...old, ride: { ...old.ride, updated_at: "t3b" }, events: [hiddenEvent({ status: "confirmed", hidden: true, time_ms: 1790000000000 })] };
  check("字幕にしないまま、時刻を確定して置ける", (await call("PUT", `/api/rides/${ID}`, { body: timed, ifMatch: "t3a" })).status, 200);
  check("確定した時刻と、字幕にしないことが、両方返る", (await call("GET", `/api/rides/${ID}`)).body.events, timed.events);

  // 書く行は、変わったものだけにする。数は、走行の 1 行と、変わった経由点・イベントの行と、その索引。
  const big = (updated_at, change = (events) => events) => ({
    ride: { ...document(updated_at).ride },
    waypoints: Array.from({ length: 40 }, (_, i) => ({ lon: 138 + i / 100, lat: 36, segment_mode: "straight" })),
    route: { segments: Array.from({ length: 39 }, () => []) },
    events: change(
      Array.from({ length: 300 }, (_, i) => ({
        id: `0199c0de-0000-7000-8000-${String(i).padStart(12, "0")}`,
        kind: "tunnel",
        label: `トンネル ${i}`,
        route_m: i * 100,
        time_ms: null,
        status: "estimated",
        detail: { end_m: i * 100 + 50, extracted_label: `トンネル ${i}` },
      })),
    ),
  });
  const written = async (body, ifMatch) => {
    const response = await call("PUT", `/api/rides/${ID}`, { body, ifMatch });
    return response.status === 200 ? response.body.rows_written : `status ${response.status}`;
  };
  const first = await written(big("t4"), "t3b");
  const same = await written(big("t5"), "t4");
  const confirmOne = (events) => events.map((e, i) => (i === 150 ? { ...e, status: "confirmed", time_ms: 1790000000000 } : e));
  const one = await written(big("t6", confirmOne), "t5");
  const dropOne = (events) => confirmOne(events).filter((_, i) => i !== 10);
  const shifted = await written(big("t7", dropOne), "t6");
  console.log(`     書いた行数: 初めて置く ${first}、何も変えずに置く ${same}、イベント 1 件を確定 ${one}、手前の 1 件を消す ${shifted}`);
  check("何も変えずに置くと、ほとんど書かない", same <= 2, true);
  check("イベント 1 件を確定しても、書くのは数行", one <= 6, true);
  check("変えたものが返る", (await call("GET", `/api/rides/${ID}`)).body, big("t7", dropOne));
  check("大きな走行を、空に戻せる", (await call("PUT", `/api/rides/${ID}`, { body: { ...shorter, ride: { ...shorter.ride, updated_at: "t3" } }, ifMatch: "t7" })).status, 200);

  const wrongId = { ...document("t4"), ride: { ...document("t4").ride, id: "0199c0de-0000-7000-8000-00000000000b" } };
  check("パスと中身の ID が違えば 400", (await call("PUT", `/api/rides/${ID}`, { body: wrongId, ifMatch: "t3" })).status, 400);
  const broken = { ...document("t4"), route: { segments: [] } };
  check("形の正しくない走行は 400", (await call("PUT", `/api/rides/${ID}`, { body: broken, ifMatch: "t3" })).status, 400);

  check("消せる", (await call("DELETE", `/api/rides/${ID}`)).status, 204);
  check("消したものは 404", (await call("GET", `/api/rides/${ID}`)).status, 404);
} catch (e) {
  failed += 1;
  console.log(`NG   ${e}`);
} finally {
  certs.close();
}
console.log(failed === 0 ? "すべて通りました" : `${failed} 件が通りません`);
process.exit(failed === 0 ? 0 : 1);
