//! 走行ログの API。
//!
//! | メソッド | パス | 内容 |
//! | --- | --- | --- |
//! | GET | /api/rides | 走行の一覧 |
//! | GET | /api/rides/:id | 走行の全データ |
//! | PUT | /api/rides/:id | 走行の全データを置き換える。`If-Match` に、手元が知っている更新時刻を入れる |
//! | DELETE | /api/rides/:id | 走行を消す |
//!
//! どの経路も、Cloudflare Access のトークンを検証してから応える。

mod access;
mod rides;

use worker::{Context, Env, Method, Request, Response, Result, event};

/// 応えを返せなかった理由。そのまま状態コードと本文になる。
pub struct Failure {
    status: u16,
    message: String,
}

impl Failure {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

/// D1 や R2 が失敗した。中身は外へ出さず、ログにだけ残す。
impl From<worker::Error> for Failure {
    fn from(error: worker::Error) -> Self {
        worker::console_error!("{error}");
        Self::new(500, "サーバの中で失敗しました")
    }
}

fn message(status: u16, text: &str) -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({ "error": text }))?.with_status(status))
}

/// 走行の ID は UUID の形だけを受け付ける。R2 のキーに使うためである。
fn is_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}

async fn route(mut req: Request, env: &Env) -> std::result::Result<Response, Failure> {
    let path = req.path();
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.first() != Some(&"api") {
        return Err(Failure::new(404, "そのパスはありません"));
    }
    access::authorize(&req, env).await?;
    match (req.method(), &segments[1..]) {
        (Method::Get, ["rides"]) => rides::list(env).await,
        (method, ["rides", id]) if is_id(id) => match method {
            Method::Get => rides::get(env, id).await,
            Method::Put => rides::put(&mut req, env, id).await,
            Method::Delete => rides::delete(env, id).await,
            _ => Err(Failure::new(405, "そのメソッドは使えません")),
        },
        _ => Err(Failure::new(404, "そのパスはありません")),
    }
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    match route(req, &env).await {
        Ok(response) => Ok(response),
        Err(failure) => message(failure.status, &failure.message),
    }
}
