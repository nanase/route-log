//! クラウドとの同期。走行を丸ごと送り、丸ごと受け取る。
//!
//! 同期は自動で行う。いつ送り、いつ受け取るかは画面の側が決める。ここは、手元とクラウドの
//! 突き合わせと、走行 1 つ分の送り受けを受け持つ。手元とクラウドのどちらが新しいかは、
//! 最後に同期したときの更新時刻を覚えておいて見分ける。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use routelog_core::RideDocument;
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::config::ConfigFile;
use crate::store::{RideSummary, Store};

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
    /// 手元だけが変わった。
    LocalNewer,
    /// クラウドだけが変わった。
    RemoteNewer,
    /// 手元にしか無い。
    LocalOnly,
    /// クラウドにしか無い。
    RemoteOnly,
    /// 両方が変わった。どちらを残すかは人が決める。
    Conflict,
}

/// 突き合わせた走行に、自動ですること。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// 何もしない。
    Keep,
    /// 手元の走行を送る。
    Push,
    /// クラウドの走行を受け取る。
    Pull,
    /// どちらを残すかを人に聞く。勝手には上書きしない。
    Ask,
}

impl SyncState {
    pub fn action(self) -> Action {
        match self {
            Self::Same => Action::Keep,
            Self::LocalNewer | Self::LocalOnly => Action::Push,
            Self::RemoteNewer | Self::RemoteOnly => Action::Pull,
            Self::Conflict => Action::Ask,
        }
    }
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

/// 突き合わせに使う、走行の見出し。手元の一覧からも、クラウドの一覧からも作る。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Head {
    id: String,
    title: String,
    ride_date: String,
    updated_at: String,
}

impl From<RideSummary> for Head {
    fn from(ride: RideSummary) -> Self {
        Self {
            id: ride.id,
            title: ride.title,
            ride_date: ride.ride_date,
            updated_at: ride.updated_at,
        }
    }
}

/// 突き合わせた走行の 1 行。
#[derive(Debug, PartialEq, Serialize)]
pub struct SyncRow {
    id: String,
    title: String,
    ride_date: String,
    local_updated_at: Option<String>,
    remote_updated_at: Option<String>,
    state: SyncState,
    action: Action,
}

/// 手元とクラウドの走行を突き合わせ、走行ごとの関係と、自動ですることを決める。日付の新しい順。
///
/// 通信も、ファイルの読み書きもしない。`synced` は、走行ごとの、最後に同期したときの更新時刻である。
pub fn reconcile(
    local: &[Head],
    remote: &[Head],
    synced: &HashMap<String, String>,
) -> Vec<SyncRow> {
    let mut rows: Vec<SyncRow> = Vec::new();
    for ride in local {
        let there = remote.iter().find(|r| r.id == ride.id);
        let state = classify(
            Some(&ride.updated_at),
            there.map(|r| r.updated_at.as_str()),
            synced.get(&ride.id).map(String::as_str),
        )
        .unwrap_or(SyncState::LocalOnly);
        rows.push(SyncRow {
            id: ride.id.clone(),
            title: ride.title.clone(),
            ride_date: ride.ride_date.clone(),
            local_updated_at: Some(ride.updated_at.clone()),
            remote_updated_at: there.map(|r| r.updated_at.clone()),
            state,
            action: state.action(),
        });
    }
    for ride in remote {
        if !local.iter().any(|l| l.id == ride.id) {
            rows.push(SyncRow {
                id: ride.id.clone(),
                title: ride.title.clone(),
                ride_date: ride.ride_date.clone(),
                local_updated_at: None,
                remote_updated_at: Some(ride.updated_at.clone()),
                state: SyncState::RemoteOnly,
                action: SyncState::RemoteOnly.action(),
            });
        }
    }
    rows.sort_by(|a, b| (&b.ride_date, &b.id).cmp(&(&a.ride_date, &a.id)));
    rows
}

/// クラウドから消せなくても、手元の走行を消してよいか。
///
/// 手元にあって 1 度も同期していない走行は、クラウドに無い。繋がらないときでも消せる。
fn never_sent(local: bool, synced: Option<&str>) -> bool {
    local && synced.is_none()
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

    fn all(&self) -> HashMap<String, String> {
        self.map.lock().unwrap().clone()
    }

    /// 覚えが同じなら、何も書かない。
    fn set(&self, id: &str, updated_at: &str) -> Result<(), String> {
        let mut map = self.map.lock().unwrap();
        if map.get(id).is_some_and(|known| known == updated_at) {
            return Ok(());
        }
        map.insert(id.to_owned(), updated_at.to_owned());
        self.write(&map)
    }

    /// 走行を消したときに、同期の覚えも消す。覚えが無ければ、何も書かない。
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let mut map = self.map.lock().unwrap();
        if map.remove(id).is_none() {
            return Ok(());
        }
        self.write(&map)
    }

    fn write(&self, map: &HashMap<String, String>) -> Result<(), String> {
        let json = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
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
    // 自動の確認の間は、本物のサービストークンを読まない。
    if crate::check::data_dir().is_some() {
        return None;
    }
    let stored = keyring_core::Entry::new(SERVICE, TOKEN)
        .ok()?
        .get_password()
        .ok()?;
    let (id, secret) = stored.split_once('\n')?;
    Some((id.to_owned(), secret.to_owned()))
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
        // 繋がらないことは早く知りたいので、繋ぐまでの待ち時間は短くする。
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
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

    fn list(&self) -> Result<Vec<Head>, String> {
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

    /// 走行を消す。クラウドにもう無ければ、消えたものとして扱う。
    fn delete(&self, id: &str) -> Result<(), String> {
        match self.request("DELETE", &format!("/api/rides/{id}")).call() {
            Ok(_) | Err(ureq::Error::Status(404, _)) => Ok(()),
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
        has_token: std::env::var_os(JWT_ENV).is_some() || token().is_some(),
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

/// 手元とクラウドの走行を突き合わせた一覧。それぞれに、自動ですることが付く。
#[tauri::command]
pub async fn sync_list(app: tauri::AppHandle) -> Result<Vec<SyncRow>, String> {
    blocking(&app, |app| {
        let api = Api::new(&app.state::<ConfigFile>())?;
        // 手元と覚えを先に読む。クラウドの一覧を待つ間に送信が済んでも、「同じ」に見える。
        // 逆の順だと、送ったばかりの走行が「クラウドが新しい」に見えて、受け取り直してしまう。
        let local = app.state::<Store>().list().map_err(|e| e.to_string())?;
        let local: Vec<Head> = local.into_iter().map(Head::from).collect();
        let synced = app.state::<Synced>();
        let known = synced.all();
        let remote = api.list()?;
        let rows = reconcile(&local, &remote, &known);
        // 同じ内容なら、同期した覚えを合わせておく。送ったあとの応答を受け取れなかったときに、
        // 覚えだけが古いまま残る。そのまま手元で直すと、「両方で変更」に見えてしまう。
        for row in rows.iter().filter(|row| row.state == SyncState::Same) {
            if let Some(updated_at) = &row.local_updated_at {
                synced.set(&row.id, updated_at)?;
            }
        }
        Ok(rows)
    })
    .await
}

/// 送った結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pushed {
    Sent,
    /// クラウドの走行が、別の場所で書き換えられていた。送っていない。
    Conflict,
}

/// 手元の走行をクラウドへ送る。
///
/// クラウドが別の場所で書き換えられていたら、送らずに `Conflict` を返す。`overwrite` なら、
/// それでも手元の内容で置き換える。
///
/// 推定した時刻は送らない。クラウドは、変わったイベントの行だけを書く。推定まで送ると、時刻を
/// 1 つ確定するたびに、まわりのイベントがみな変わったことになる。
#[tauri::command]
pub async fn sync_push(
    app: tauri::AppHandle,
    id: String,
    overwrite: bool,
) -> Result<Pushed, String> {
    blocking(&app, move |app| {
        let api = Api::new(&app.state::<ConfigFile>())?;
        let mut doc = app.state::<Store>().load(&id).map_err(|e| e.to_string())?;
        routelog_core::clear_estimates(&mut doc);
        let synced = app.state::<Synced>();
        let mut outcome = api.put(&doc, synced.get(&id).as_deref())?;
        if let (Err(theirs), true) = (&outcome, overwrite) {
            outcome = api.put(&doc, Some(theirs))?;
        }
        match outcome {
            Ok(()) => {
                synced.set(&id, &doc.ride.updated_at)?;
                Ok(Pushed::Sent)
            }
            Err(_) => Ok(Pushed::Conflict),
        }
    })
    .await
}

/// クラウドの走行を取ってくる。手元にはまだ置かない。置くのは `sync_apply` である。
///
/// 取るのと置くのを分けてあるのは、画面の側が、編集の合間を選んで置くためである。
///
/// クラウドには推定した時刻が無いので、ここで求め直す。
#[tauri::command]
pub async fn sync_fetch(app: tauri::AppHandle, id: String) -> Result<RideDocument, String> {
    blocking(&app, move |app| {
        let mut doc = Api::new(&app.state::<ConfigFile>())?.get(&id)?;
        routelog_core::refresh(&mut doc);
        Ok(doc)
    })
    .await
}

/// 取ってきた走行を手元に置く。置いたら `true`。
///
/// 手元の走行の更新時刻が `expect` と違えば、取ってくる間に手元も変わっている。置かずに
/// `false` を返す。`expect` が無ければ、手元に無いことを確かめる。`overwrite` なら、確かめずに置く。
#[tauri::command]
pub fn sync_apply(
    store: tauri::State<Store>,
    synced: tauri::State<Synced>,
    doc: RideDocument,
    expect: Option<String>,
    overwrite: bool,
) -> Result<bool, String> {
    let placed = if overwrite {
        store.save(&doc).map(|()| true)
    } else {
        store.replace_if(&doc, expect.as_deref())
    }
    .map_err(|e| e.to_string())?;
    if placed {
        synced.set(&doc.ride.id, &doc.ride.updated_at)?;
    }
    Ok(placed)
}

/// クラウドの走行を消す。同期の覚えも消す。手元の走行には触らない。
///
/// 消せなければエラーにする。呼んだ側は、手元の走行を消さずに残す。ただし、1 度も同期して
/// いない走行はクラウドに無いので、消せなくてもエラーにしない。
#[tauri::command]
pub async fn sync_delete(app: tauri::AppHandle, id: String) -> Result<(), String> {
    blocking(&app, move |app| {
        let synced = app.state::<Synced>();
        let local = app.state::<Store>().load(&id).is_ok();
        let deleted = Api::new(&app.state::<ConfigFile>()).and_then(|api| api.delete(&id));
        if !never_sent(local, synced.get(&id).as_deref()) {
            deleted?;
        }
        synced.remove(&id)
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
        // 覚えが、どちらとも違っていても同じである。
        assert_eq!(
            classify(Some("b"), Some("b"), Some("a")),
            Some(SyncState::Same)
        );
    }

    #[test]
    fn a_ride_the_cloud_no_longer_has_counts_as_only_here() {
        // 同期した覚えがあっても、クラウドに無ければ「手元だけ」になる。
        assert_eq!(
            classify(Some("a"), None, Some("a")),
            Some(SyncState::LocalOnly)
        );
        assert_eq!(
            classify(None, Some("a"), Some("a")),
            Some(SyncState::RemoteOnly)
        );
    }

    #[test]
    fn only_a_ride_changed_on_both_sides_is_left_to_a_person() {
        use SyncState::*;
        for (state, action) in [
            (Same, Action::Keep),
            (LocalNewer, Action::Push),
            (LocalOnly, Action::Push),
            (RemoteNewer, Action::Pull),
            (RemoteOnly, Action::Pull),
            (Conflict, Action::Ask),
        ] {
            assert_eq!(state.action(), action, "{state:?}");
        }
    }

    fn head(id: &str, ride_date: &str, updated_at: &str) -> Head {
        Head {
            id: id.into(),
            title: format!("{id} の題"),
            ride_date: ride_date.into(),
            updated_at: updated_at.into(),
        }
    }

    /// 突き合わせの結果から、走行の ID・関係・することだけを取り出す。
    fn decided(rows: &[SyncRow]) -> Vec<(&str, SyncState, Action)> {
        rows.iter()
            .map(|row| (row.id.as_str(), row.state, row.action))
            .collect()
    }

    #[test]
    fn every_ride_on_either_side_gets_one_decision() {
        use SyncState::*;
        let local = [
            head("same", "2026-10-01", "t1"),
            head("edited-here", "2026-10-02", "t2"),
            head("edited-there", "2026-10-03", "t1"),
            head("new-here", "2026-10-04", "t1"),
            head("edited-both", "2026-10-05", "t2"),
        ];
        let remote = [
            head("same", "2026-10-01", "t1"),
            head("edited-here", "2026-10-02", "t1"),
            head("edited-there", "2026-10-03", "t3"),
            head("edited-both", "2026-10-05", "t3"),
            head("new-there", "2026-10-06", "t1"),
        ];
        let synced = ["same", "edited-here", "edited-there", "edited-both", "gone"]
            .map(|id| (id.to_owned(), "t1".to_owned()))
            .into();
        let rows = reconcile(&local, &remote, &synced);
        assert_eq!(
            decided(&rows),
            [
                ("new-there", RemoteOnly, Action::Pull),
                ("edited-both", Conflict, Action::Ask),
                ("new-here", LocalOnly, Action::Push),
                ("edited-there", RemoteNewer, Action::Pull),
                ("edited-here", LocalNewer, Action::Push),
                ("same", Same, Action::Keep),
            ],
            "日付の新しい順に並び、どちらにも無い走行は出ない"
        );
        let there = &rows[0];
        assert_eq!(there.title, "new-there の題");
        assert_eq!(there.local_updated_at, None);
        assert_eq!(there.remote_updated_at.as_deref(), Some("t1"));
        let both = &rows[1];
        assert_eq!(both.local_updated_at.as_deref(), Some("t2"));
        assert_eq!(both.remote_updated_at.as_deref(), Some("t3"));
    }

    #[test]
    fn a_new_place_receives_everything_and_sends_nothing() {
        let remote = [head("a", "2026-10-01", "t1"), head("b", "2026-10-02", "t2")];
        let rows = reconcile(&[], &remote, &HashMap::new());
        assert_eq!(
            decided(&rows),
            [
                ("b", SyncState::RemoteOnly, Action::Pull),
                ("a", SyncState::RemoteOnly, Action::Pull),
            ]
        );
    }

    #[test]
    fn an_empty_cloud_is_sent_everything_even_what_was_synced_before() {
        // クラウドの側が空になっても、手元の走行は消さない。送り直す。
        let local = [head("a", "2026-10-01", "t1"), head("b", "2026-10-02", "t2")];
        let synced = [("a".to_owned(), "t1".to_owned())].into();
        let rows = reconcile(&local, &[], &synced);
        assert_eq!(
            decided(&rows),
            [
                ("b", SyncState::LocalOnly, Action::Push),
                ("a", SyncState::LocalOnly, Action::Push),
            ]
        );
    }

    #[test]
    fn nothing_is_done_when_both_sides_are_the_same() {
        let rides = [head("a", "2026-10-01", "t1"), head("b", "2026-10-02", "t2")];
        let rows = reconcile(&rides, &rides, &HashMap::new());
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.action == Action::Keep));
        assert_eq!(reconcile(&[], &[], &HashMap::new()), []);
    }

    #[test]
    fn a_ride_that_was_never_sent_can_be_removed_without_the_cloud() {
        assert!(never_sent(true, None));
        assert!(
            !never_sent(true, Some("t1")),
            "送ってある走行は、クラウドから先に消す"
        );
        assert!(
            !never_sent(false, None),
            "クラウドにだけある走行は、クラウドから消す"
        );
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

    #[test]
    fn a_removed_record_of_syncing_stays_removed() {
        let dir =
            std::env::temp_dir().join(format!("routelog-synced-remove-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sync.json");
        let synced = Synced::open(path.clone());
        synced.remove("x").unwrap();
        assert!(!path.exists(), "覚えが無ければ、ファイルを作らない");
        synced.set("x", "2026-10-03T05:00:00+09:00").unwrap();
        synced.set("y", "2026-10-04T05:00:00+09:00").unwrap();
        synced.remove("x").unwrap();
        let reopened = Synced::open(path);
        assert_eq!(reopened.get("x"), None);
        assert_eq!(
            reopened.get("y").as_deref(),
            Some("2026-10-04T05:00:00+09:00")
        );
        assert_eq!(reopened.all().len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
