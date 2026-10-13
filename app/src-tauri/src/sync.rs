//! クラウドとの同期。走行を丸ごと送り、丸ごと受け取る。
//!
//! 同期は人が押したときにだけ行う。手元とクラウドのどちらが新しいかは、最後に同期した
//! ときの更新時刻を覚えておいて見分ける。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use routelog_core::RideDocument;
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::config::ConfigFile;
use crate::store::Store;

/// 資格情報ストアでの名前。
const SERVICE: &str = "routelog";
const TOKEN: &str = "access-service-token";
/// 手元で動かした Worker に繋ぐとき、Access の代わりにトークンをそのまま渡すための環境変数。
const JWT_ENV: &str = "ROUTELOG_SYNC_JWT";

/// 手元とクラウドの、走行 1 つの関係。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    /// 同じ内容。
    Same,
    /// 手元だけが変わった。送ればよい。
    LocalNewer,
    /// クラウドだけが変わった。受け取ればよい。
    RemoteNewer,
    /// 手元にしか無い。
    LocalOnly,
    /// クラウドにしか無い。
    RemoteOnly,
    /// 両方が変わった。どちらを残すかは人が決める。
    Conflict,
}

/// 手元・クラウド・最後に同期したときの更新時刻から、関係を決める。どちらにも無ければ `None`。
pub fn classify(
    local: Option<&str>,
    remote: Option<&str>,
    synced: Option<&str>,
) -> Option<SyncState> {
    Some(match (local, remote) {
        (None, None) => return None,
        (Some(_), None) => SyncState::LocalOnly,
        (None, Some(_)) => SyncState::RemoteOnly,
        (Some(local), Some(remote)) if local == remote => SyncState::Same,
        (Some(_), Some(remote)) if synced == Some(remote) => SyncState::LocalNewer,
        (Some(local), Some(_)) if synced == Some(local) => SyncState::RemoteNewer,
        (Some(_), Some(_)) => SyncState::Conflict,
    })
}

/// 走行ごとの、最後に同期したときの更新時刻。
pub struct Synced {
    path: PathBuf,
    map: Mutex<HashMap<String, String>>,
}

impl Synced {
    /// 読めないときは、何も同期していない状態から始める。
    pub fn open(path: PathBuf) -> Self {
        let map = std::fs::read_to_string(&path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        Self {
            path,
            map: Mutex::new(map),
        }
    }

    fn get(&self, id: &str) -> Option<String> {
        self.map.lock().unwrap().get(id).cloned()
    }

    fn set(&self, id: &str, updated_at: &str) -> Result<(), String> {
        let mut map = self.map.lock().unwrap();
        map.insert(id.to_owned(), updated_at.to_owned());
        let json = serde_json::to_string_pretty(&*map).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, json).map_err(|e| format!("{}: {e}", self.path.display()))
    }
}

/// 資格情報ストアを使えるようにする。アプリの起動時に 1 度呼ぶ。
pub fn init_credentials() {
    #[cfg(windows)]
    if let Ok(store) = windows_native_keyring_store::Store::new() {
        keyring_core::set_default_store(store);
    }
}

fn token() -> Option<(String, String)> {
    let stored = keyring_core::Entry::new(SERVICE, TOKEN)
        .ok()?
        .get_password()
        .ok()?;
    let (id, secret) = stored.split_once('\n')?;
    Some((id.to_owned(), secret.to_owned()))
}

#[derive(Deserialize)]
struct Remote {
    id: String,
    title: String,
    ride_date: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct Stamp {
    updated_at: String,
}

/// API の呼び出し口。
struct Api {
    base: String,
    headers: Vec<(&'static str, String)>,
    agent: ureq::Agent,
}

impl Api {
    fn new(config: &ConfigFile) -> Result<Self, String> {
        let base = config
            .get()
            .api_url
            .filter(|url| !url.is_empty())
            .ok_or("API の URL が設定されていません")?;
        let headers = match std::env::var(JWT_ENV).ok().filter(|v| !v.is_empty()) {
            Some(jwt) => vec![("Cf-Access-Jwt-Assertion", jwt)],
            None => {
                let (id, secret) = token().ok_or("サービストークンが設定されていません")?;
                vec![
                    ("CF-Access-Client-Id", id),
                    ("CF-Access-Client-Secret", secret),
                ]
            }
        };
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .build();
        Ok(Self {
            base: base.trim_end_matches('/').to_owned(),
            headers,
            agent,
        })
    }

    fn request(&self, method: &str, path: &str) -> ureq::Request {
        let request = self.agent.request(method, &format!("{}{path}", self.base));
        self.headers
            .iter()
            .fold(request, |request, (name, value)| request.set(name, value))
    }

    fn list(&self) -> Result<Vec<Remote>, String> {
        let response = self.request("GET", "/api/rides").call().map_err(explain)?;
        response.into_json().map_err(|e| e.to_string())
    }

    fn get(&self, id: &str) -> Result<RideDocument, String> {
        let response = self
            .request("GET", &format!("/api/rides/{id}"))
            .call()
            .map_err(explain)?;
        response.into_json().map_err(|e| e.to_string())
    }

    /// 走行を置く。クラウドの更新時刻が `known` と違えば置かず、クラウドの更新時刻を `Err` で返す。
    fn put(&self, doc: &RideDocument, known: Option<&str>) -> Result<Result<(), String>, String> {
        let mut request = self.request("PUT", &format!("/api/rides/{}", doc.ride.id));
        if let Some(known) = known {
            request = request.set("If-Match", known);
        }
        match request.send_json(doc) {
            Ok(_) => Ok(Ok(())),
            Err(ureq::Error::Status(412, response)) => {
                let stamp: Stamp = response.into_json().map_err(|e| e.to_string())?;
                Ok(Err(stamp.updated_at))
            }
            Err(error) => Err(explain(error)),
        }
    }
}

fn explain(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(403, _) => {
            "クラウドに拒まれました。サービストークンを確かめてください".into()
        }
        ureq::Error::Status(code, _) => format!("クラウドが {code} を返しました"),
        ureq::Error::Transport(transport) => format!("クラウドに繋がりません: {transport}"),
    }
}

/// 設定の画面に出す、いまの状態。トークンそのものは返さない。
#[derive(Serialize)]
pub struct SyncSettings {
    api_url: Option<String>,
    has_token: bool,
}

#[tauri::command]
pub fn get_sync_settings(config: tauri::State<ConfigFile>) -> SyncSettings {
    SyncSettings {
        api_url: config.get().api_url,
        has_token: token().is_some() || std::env::var_os(JWT_ENV).is_some(),
    }
}

/// API の URL と、Access のサービストークンを覚える。トークンは OS の資格情報ストアに置く。
/// トークンの欄が空なら、いまのトークンをそのまま使う。
#[tauri::command]
pub fn set_sync_settings(
    config: tauri::State<ConfigFile>,
    api_url: String,
    client_id: String,
    client_secret: String,
) -> Result<(), String> {
    let api_url = api_url.trim().to_owned();
    if !api_url.is_empty()
        && !api_url.starts_with("https://")
        && !api_url.starts_with("http://127.0.0.1")
    {
        return Err("API の URL は https:// で始めてください".into());
    }
    config.update(|c| c.api_url = Some(api_url).filter(|url| !url.is_empty()))?;
    let (id, secret) = (client_id.trim(), client_secret.trim());
    if !id.is_empty() && !secret.is_empty() {
        keyring_core::Entry::new(SERVICE, TOKEN)
            .and_then(|entry| entry.set_password(&format!("{id}\n{secret}")))
            .map_err(|e| format!("資格情報ストアに保存できません: {e}"))?;
    }
    Ok(())
}

/// 同期の一覧の 1 行。
#[derive(Serialize)]
pub struct SyncRow {
    id: String,
    title: String,
    ride_date: String,
    local_updated_at: Option<String>,
    remote_updated_at: Option<String>,
    state: SyncState,
}

fn blocking<T: Send + 'static>(
    app: &tauri::AppHandle,
    work: impl FnOnce(&tauri::AppHandle) -> Result<T, String> + Send + 'static,
) -> impl Future<Output = Result<T, String>> {
    let app = app.clone();
    async move {
        tauri::async_runtime::spawn_blocking(move || work(&app))
            .await
            .map_err(|e| e.to_string())?
    }
}

/// 手元とクラウドの走行を突き合わせた一覧。日付の新しい順。
#[tauri::command]
pub async fn sync_list(app: tauri::AppHandle) -> Result<Vec<SyncRow>, String> {
    blocking(&app, |app| {
        let remote = Api::new(&app.state::<ConfigFile>())?.list()?;
        let local = app.state::<Store>().list().map_err(|e| e.to_string())?;
        let synced = app.state::<Synced>();
        let mut rows: Vec<SyncRow> = Vec::new();
        for ride in &local {
            let there = remote.iter().find(|r| r.id == ride.id);
            let remote_updated_at = there.map(|r| r.updated_at.clone());
            let state = classify(
                Some(&ride.updated_at),
                remote_updated_at.as_deref(),
                synced.get(&ride.id).as_deref(),
            );
            rows.push(SyncRow {
                id: ride.id.clone(),
                title: ride.title.clone(),
                ride_date: ride.ride_date.clone(),
                local_updated_at: Some(ride.updated_at.clone()),
                remote_updated_at,
                state: state.unwrap_or(SyncState::LocalOnly),
            });
        }
        for ride in remote {
            if !local.iter().any(|l| l.id == ride.id) {
                rows.push(SyncRow {
                    id: ride.id,
                    title: ride.title,
                    ride_date: ride.ride_date,
                    local_updated_at: None,
                    remote_updated_at: Some(ride.updated_at),
                    state: SyncState::RemoteOnly,
                });
            }
        }
        rows.sort_by(|a, b| (&b.ride_date, &b.id).cmp(&(&a.ride_date, &a.id)));
        Ok(rows)
    })
    .await
}

/// 手元の走行をクラウドへ送る。
///
/// クラウドが別の場所で書き換えられていたら送らず、エラーにする。`overwrite` なら、
/// それでも手元の内容で置き換える。
#[tauri::command]
pub async fn sync_push(app: tauri::AppHandle, id: String, overwrite: bool) -> Result<(), String> {
    blocking(&app, move |app| {
        let api = Api::new(&app.state::<ConfigFile>())?;
        let doc = app.state::<Store>().load(&id).map_err(|e| e.to_string())?;
        let synced = app.state::<Synced>();
        let mut outcome = api.put(&doc, synced.get(&id).as_deref())?;
        if let (Err(theirs), true) = (&outcome, overwrite) {
            outcome = api.put(&doc, Some(theirs))?;
        }
        match outcome {
            Ok(()) => synced.set(&id, &doc.ride.updated_at),
            Err(_) => Err("クラウドの走行が、別の場所で書き換えられています".into()),
        }
    })
    .await
}

/// クラウドの走行を受け取り、手元の走行を置き換える。
#[tauri::command]
pub async fn sync_pull(app: tauri::AppHandle, id: String) -> Result<(), String> {
    blocking(&app, move |app| {
        let doc = Api::new(&app.state::<ConfigFile>())?.get(&id)?;
        app.state::<Store>().save(&doc).map_err(|e| e.to_string())?;
        app.state::<Synced>().set(&id, &doc.ride.updated_at)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_exists_on_one_side_only_goes_to_the_other() {
        assert_eq!(classify(Some("a"), None, None), Some(SyncState::LocalOnly));
        assert_eq!(classify(None, Some("a"), None), Some(SyncState::RemoteOnly));
        assert_eq!(classify(None, None, Some("a")), None);
    }

    #[test]
    fn the_side_that_moved_since_the_last_sync_is_the_newer_one() {
        assert_eq!(
            classify(Some("a"), Some("a"), Some("a")),
            Some(SyncState::Same)
        );
        assert_eq!(
            classify(Some("b"), Some("a"), Some("a")),
            Some(SyncState::LocalNewer)
        );
        assert_eq!(
            classify(Some("a"), Some("b"), Some("a")),
            Some(SyncState::RemoteNewer)
        );
    }

    #[test]
    fn when_both_sides_moved_a_person_decides() {
        assert_eq!(
            classify(Some("b"), Some("c"), Some("a")),
            Some(SyncState::Conflict)
        );
        // 同期した覚えが無いのに、両方にあって違う。
        assert_eq!(
            classify(Some("b"), Some("c"), None),
            Some(SyncState::Conflict)
        );
    }

    #[test]
    fn the_same_content_is_the_same_even_without_a_record_of_syncing() {
        assert_eq!(classify(Some("a"), Some("a"), None), Some(SyncState::Same));
    }

    #[test]
    fn the_record_of_syncing_is_there_the_next_time() {
        let dir = std::env::temp_dir().join(format!("routelog-synced-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sync.json");
        let synced = Synced::open(path.clone());
        assert_eq!(synced.get("x"), None);
        synced.set("x", "2026-10-03T05:00:00+09:00").unwrap();
        assert_eq!(
            Synced::open(path).get("x").as_deref(),
            Some("2026-10-03T05:00:00+09:00")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
