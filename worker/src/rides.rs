//! 走行の保存と読み出し。走行・経由点・イベントは D1 に、経路の線は R2 に置く。

use routelog_core::{Ride, RideDocument, Route, Waypoint};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

/// 送り受けする走行。イベントは中身を見ずに、送られてきた形のまま持つ。
///
/// イベントの種類と項目は、アプリの側で増える。ここで形を決めると、Worker を入れ替えるまで、
/// 新しいアプリの走行を読めずに 400 で拒むことになる。列に出す値は、SQL が JSON から取り出す。
#[derive(Serialize, Deserialize)]
struct Document {
    ride: Ride,
    #[serde(default)]
    waypoints: Vec<Waypoint>,
    #[serde(default)]
    route: Route,
    #[serde(default)]
    events: Vec<Value>,
}

/// イベントが、列に出す値を持っているか。無いと、D1 に入れるときに失敗する。
fn storable(event: &Value) -> bool {
    let text = |key: &str| event.get(key).and_then(Value::as_str);
    text("id").is_some_and(|id| !id.is_empty())
        && text("kind").is_some()
        && text("label").is_some()
        && text("status").is_some()
        && event.get("route_m").is_some_and(Value::is_number)
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
    /// ラベルの並びを JSON にしたもの。
    labels: String,
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
            "SELECT id, title, ride_date, clock_offset_ms, is_public, labels, created_at, updated_at \
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
    let events: Vec<Value> = rows
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
    Ok(Response::from_json(&Document {
        ride: Ride {
            id: ride.id,
            title: ride.title,
            ride_date: ride.ride_date,
            clock_offset_ms: ride.clock_offset_ms,
            is_public: ride.is_public != 0,
            labels: serde_json::from_str(&ride.labels).unwrap_or_default(),
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
///
/// 経由点とイベントは、変わった行だけを書く。アプリは編集のたびに走行を丸ごと送ってくるので、
/// 全部を消して入れ直すと、D1 の書き込みの行数が、編集の回数にイベントの数を掛けただけ増える。
/// 書いた行数は、確かめられるよう応答に入れて返す。
pub async fn put(req: &mut Request, env: &Env, id: &str) -> Reply {
    // 読めない理由を返す。アプリと Worker の版が食い違ったときに、何が合わないのかが分かる。
    let body: Document = req
        .json()
        .await
        .map_err(|e| Failure::new(400, format!("走行のデータを読めません: {e}")))?;
    if body.ride.id != id {
        return Err(Failure::new(400, "パスの ID と走行の ID が違います"));
    }
    if !body.events.iter().all(storable) {
        return Err(Failure::new(
            400,
            "イベントに、ID・種類・名前・位置・状態のどれかがありません",
        ));
    }
    let events = body.events;
    // 経路の形は、イベントを除いて確かめる。
    let doc = RideDocument {
        ride: body.ride,
        waypoints: body.waypoints,
        route: body.route,
        events: Vec::new(),
    };
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
    //
    // 送られてきた中に無い行を消し、ある行は、中身が違うときだけ書き換える。SELECT のあとの
    // `WHERE true` は、続く ON CONFLICT を結合の条件と読み違えさせないために要る。
    let ride = &doc.ride;
    let statements = vec![
        db.prepare(
            "INSERT INTO rides (id, title, ride_date, clock_offset_ms, is_public, route_key, created_at, updated_at, labels) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
             ON CONFLICT(id) DO UPDATE SET title = excluded.title, ride_date = excluded.ride_date, \
             clock_offset_ms = excluded.clock_offset_ms, is_public = excluded.is_public, \
             route_key = excluded.route_key, updated_at = excluded.updated_at, labels = excluded.labels",
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
            json(&ride.labels)?,
        ])?,
        db.prepare("DELETE FROM waypoints WHERE ride_id = ?1 AND seq >= json_array_length(?2)")
            .bind(&[id.into(), json(&doc.waypoints)?])?,
        db.prepare(
            "INSERT INTO waypoints (ride_id, seq, lon, lat, segment_mode) \
             SELECT ?1, key, json_extract(value, '$.lon'), json_extract(value, '$.lat'), \
             json_extract(value, '$.segment_mode') FROM json_each(?2) WHERE true \
             ON CONFLICT(ride_id, seq) DO UPDATE SET lon = excluded.lon, lat = excluded.lat, \
             segment_mode = excluded.segment_mode \
             WHERE waypoints.lon IS NOT excluded.lon OR waypoints.lat IS NOT excluded.lat \
             OR waypoints.segment_mode IS NOT excluded.segment_mode",
        )
        .bind(&[id.into(), json(&doc.waypoints)?])?,
        db.prepare(
            "DELETE FROM events WHERE ride_id = ?1 \
             AND id NOT IN (SELECT json_extract(value, '$.id') FROM json_each(?2))",
        )
        .bind(&[id.into(), json(&events)?])?,
        // ほかの走行が同じ ID のイベントを持っていても、それは書き換えない。
        db.prepare(
            "INSERT INTO events (id, ride_id, seq, kind, label, route_m, time_ms, status, json) \
             SELECT json_extract(value, '$.id'), ?1, key, json_extract(value, '$.kind'), \
             json_extract(value, '$.label'), json_extract(value, '$.route_m'), \
             json_extract(value, '$.time_ms'), json_extract(value, '$.status'), value \
             FROM json_each(?2) WHERE true \
             ON CONFLICT(id) DO UPDATE SET seq = excluded.seq, kind = excluded.kind, \
             label = excluded.label, route_m = excluded.route_m, time_ms = excluded.time_ms, \
             status = excluded.status, json = excluded.json \
             WHERE events.ride_id = excluded.ride_id \
             AND (events.seq IS NOT excluded.seq OR events.json IS NOT excluded.json)",
        )
        .bind(&[id.into(), json(&events)?])?,
    ];
    let mut rows_written = 0;
    for result in db.batch(statements).await? {
        rows_written += result
            .meta()?
            .and_then(|meta| meta.rows_written)
            .unwrap_or(0);
    }
    Ok(Response::from_json(&serde_json::json!({
        "updated_at": ride.updated_at,
        "rows_written": rows_written,
    }))?)
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
