//! 走行の保存と読み出し。走行・経由点・イベントは D1 に、経路の線は R2 に置く。

use routelog_core::{Event, Ride, RideDocument, Route, Waypoint};
use serde::{Deserialize, Serialize};
use worker::wasm_bindgen::JsValue;
use worker::{D1Database, Env, Request, Response};

use crate::Failure;

type Reply = Result<Response, Failure>;

fn database(env: &Env) -> Result<D1Database, Failure> {
    Ok(env.d1("DB")?)
}

fn route_key(id: &str) -> String {
    format!("rides/{id}/route.json")
}

/// 一覧に出す、走行の要約。
#[derive(Serialize, Deserialize)]
struct Listed {
    id: String,
    title: String,
    ride_date: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct RideRow {
    id: String,
    title: String,
    ride_date: String,
    clock_offset_ms: i64,
    is_public: i64,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct EventRow {
    /// イベントを JSON にしたもの。保存したときの形のまま返す。
    json: String,
}

pub async fn list(env: &Env) -> Reply {
    let rides = database(env)?
        .prepare(
            "SELECT id, title, ride_date, updated_at FROM rides ORDER BY ride_date DESC, id DESC",
        )
        .all()
        .await?
        .results::<Listed>()?;
    Ok(Response::from_json(&rides)?)
}

async fn updated_at(db: &D1Database, id: &str) -> Result<Option<String>, Failure> {
    let statement = db
        .prepare("SELECT updated_at FROM rides WHERE id = ?1")
        .bind(&[id.into()])?;
    Ok(statement.first::<String>(Some("updated_at")).await?)
}

pub async fn get(env: &Env, id: &str) -> Reply {
    let db = database(env)?;
    let ride = db
        .prepare(
            "SELECT id, title, ride_date, clock_offset_ms, is_public, created_at, updated_at \
             FROM rides WHERE id = ?1",
        )
        .bind(&[id.into()])?
        .first::<RideRow>(None)
        .await?
        .ok_or_else(|| Failure::new(404, "その走行はありません"))?;
    let waypoints = db
        .prepare("SELECT lon, lat, segment_mode FROM waypoints WHERE ride_id = ?1 ORDER BY seq")
        .bind(&[id.into()])?
        .all()
        .await?
        .results::<Waypoint>()?;
    let rows = db
        .prepare("SELECT json FROM events WHERE ride_id = ?1 ORDER BY route_m, seq")
        .bind(&[id.into()])?
        .all()
        .await?
        .results::<EventRow>()?;
    let events: Vec<Event> = rows
        .iter()
        .filter_map(|row| serde_json::from_str(&row.json).ok())
        .collect();
    let route = match env.bucket("ROUTES")?.get(route_key(id)).execute().await? {
        Some(object) => match object.body() {
            Some(body) => serde_json::from_str(&body.text().await?).unwrap_or_default(),
            None => Route::default(),
        },
        None => Route::default(),
    };
    Ok(Response::from_json(&RideDocument {
        ride: Ride {
            id: ride.id,
            title: ride.title,
            ride_date: ride.ride_date,
            clock_offset_ms: ride.clock_offset_ms,
            is_public: ride.is_public != 0,
            created_at: ride.created_at,
            updated_at: ride.updated_at,
        },
        waypoints,
        route,
        events,
    })?)
}

/// 走行を丸ごと置き換える。
///
/// すでにある走行は、`If-Match` がサーバの持つ更新時刻と同じときだけ置き換える。別の場所で
/// 書き換えられた走行を、古い内容で上書きしないためである。
pub async fn put(req: &mut Request, env: &Env, id: &str) -> Reply {
    let doc: RideDocument = req
        .json()
        .await
        .map_err(|_| Failure::new(400, "走行のデータを読めません"))?;
    if doc.ride.id != id {
        return Err(Failure::new(400, "パスの ID と走行の ID が違います"));
    }
    doc.validate()
        .map_err(|reason| Failure::new(400, reason.to_string()))?;

    let db = database(env)?;
    let known = req.headers().get("If-Match").ok().flatten();
    if let Some(stored) = updated_at(&db, id).await?
        && known.as_deref() != Some(stored.as_str())
    {
        let body = serde_json::json!({
            "error": "この走行は、別の場所で書き換えられています",
            "updated_at": stored,
        });
        return Ok(Response::from_json(&body)?.with_status(412));
    }

    env.bucket("ROUTES")?
        .put(
            route_key(id),
            serde_json::to_string(&doc.route).unwrap_or_default(),
        )
        .execute()
        .await?;

    // 経由点とイベントは配列を JSON で 1 つの値として渡し、json_each で行に開く。
    // 1 行ずつ入れると、1 回の呼び出しで使えるクエリの数に当たる。
    let ride = &doc.ride;
    let statements = vec![
        db.prepare(
            "INSERT INTO rides (id, title, ride_date, clock_offset_ms, is_public, route_key, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(id) DO UPDATE SET title = excluded.title, ride_date = excluded.ride_date, \
             clock_offset_ms = excluded.clock_offset_ms, is_public = excluded.is_public, \
             route_key = excluded.route_key, updated_at = excluded.updated_at",
        )
        .bind(&[
            id.into(),
            ride.title.as_str().into(),
            ride.ride_date.as_str().into(),
            (ride.clock_offset_ms as f64).into(),
            (ride.is_public as i32).into(),
            route_key(id).into(),
            ride.created_at.as_str().into(),
            ride.updated_at.as_str().into(),
        ])?,
        db.prepare("DELETE FROM waypoints WHERE ride_id = ?1")
            .bind(&[id.into()])?,
        db.prepare(
            "INSERT INTO waypoints (ride_id, seq, lon, lat, segment_mode) \
             SELECT ?1, key, json_extract(value, '$.lon'), json_extract(value, '$.lat'), \
             json_extract(value, '$.segment_mode') FROM json_each(?2)",
        )
        .bind(&[id.into(), json(&doc.waypoints)?])?,
        db.prepare("DELETE FROM events WHERE ride_id = ?1")
            .bind(&[id.into()])?,
        db.prepare(
            "INSERT INTO events (id, ride_id, seq, kind, label, route_m, time_ms, status, json) \
             SELECT json_extract(value, '$.id'), ?1, key, json_extract(value, '$.kind'), \
             json_extract(value, '$.label'), json_extract(value, '$.route_m'), \
             json_extract(value, '$.time_ms'), json_extract(value, '$.status'), value \
             FROM json_each(?2)",
        )
        .bind(&[id.into(), json(&doc.events)?])?,
    ];
    db.batch(statements).await?;
    Ok(Response::from_json(
        &serde_json::json!({ "updated_at": ride.updated_at }),
    )?)
}

pub async fn delete(env: &Env, id: &str) -> Reply {
    let db = database(env)?;
    let statements = ["events", "waypoints"]
        .map(|table| format!("DELETE FROM {table} WHERE ride_id = ?1"))
        .into_iter()
        .chain(["DELETE FROM rides WHERE id = ?1".to_owned()])
        .map(|sql| db.prepare(sql).bind(&[id.into()]))
        .collect::<worker::Result<Vec<_>>>()?;
    db.batch(statements).await?;
    env.bucket("ROUTES")?.delete(route_key(id)).await?;
    Ok(Response::empty()?.with_status(204))
}

/// 値を JSON の文字列にして、D1 に渡せる形にする。
fn json<T: Serialize>(value: &T) -> Result<JsValue, Failure> {
    let text = serde_json::to_string(value).map_err(|e| Failure::new(500, e.to_string()))?;
    Ok(JsValue::from(text))
}
