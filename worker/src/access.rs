//! Cloudflare Access のトークンの検証。鍵を取ってくる所だけがここにあり、検証そのものは
//! `routelog-access` が持つ。

use std::cell::RefCell;

use routelog_access::{Identity, Keys, verify};
use worker::{Date, Env, Fetch, Request, Url};

use crate::Failure;

/// 取ってきた鍵を使い回す時間（ミリ秒）。Access は鍵をめったに替えない。
const KEYS_CACHE_MS: u64 = 60 * 60 * 1000;

thread_local! {
    /// 鍵を取ってきた URL・時刻と、その鍵。
    static CACHE: RefCell<Option<(String, u64, Keys)>> = const { RefCell::new(None) };
}

fn refused() -> Failure {
    Failure::new(403, "この要求は受け付けられません")
}

fn setting(env: &Env, name: &str) -> Option<String> {
    let value = env
        .var(name)
        .map(|v| v.to_string())
        .or_else(|_| env.secret(name).map(|v| v.to_string()))
        .ok()?;
    (!value.is_empty()).then_some(value)
}

async fn keys(url: &str, now_ms: u64) -> Option<Keys> {
    let cached = CACHE.with_borrow(|cache| {
        let (from, at, keys) = cache.as_ref()?;
        (from == url && now_ms.saturating_sub(*at) < KEYS_CACHE_MS).then(|| keys.clone())
    });
    if cached.is_some() {
        return cached;
    }
    let mut response = Fetch::Url(Url::parse(url).ok()?).send().await.ok()?;
    if response.status_code() != 200 {
        return None;
    }
    let keys: Keys = response.json().await.ok()?;
    CACHE.set(Some((url.to_owned(), now_ms, keys.clone())));
    Some(keys)
}

/// 要求が、この API の前にある Access を通ってきたことを確かめる。
///
/// 設定が欠けているとき、鍵を取れないとき、トークンが検証を通らないときは、どれも同じ
/// 403 で拒む。理由は外へ出さない。
pub async fn authorize(req: &Request, env: &Env) -> Result<Identity, Failure> {
    let team_domain = setting(env, "ACCESS_TEAM_DOMAIN").ok_or_else(refused)?;
    let audience = setting(env, "ACCESS_AUD").ok_or_else(refused)?;
    let token = req
        .headers()
        .get("Cf-Access-Jwt-Assertion")
        .ok()
        .flatten()
        .ok_or_else(refused)?;
    // 手元で確かめるときだけ、鍵を別の場所から取れるようにする。本番の設定には置かない。
    let certs = setting(env, "ACCESS_CERTS_URL")
        .unwrap_or_else(|| format!("https://{team_domain}/cdn-cgi/access/certs"));
    let now_ms = Date::now().as_millis();
    let keys = keys(&certs, now_ms).await.ok_or_else(refused)?;
    verify(
        &token,
        &keys,
        &team_domain,
        &audience,
        (now_ms / 1000) as i64,
    )
    .map_err(|reason| {
        worker::console_warn!("トークンを拒みました: {reason}");
        refused()
    })
}
