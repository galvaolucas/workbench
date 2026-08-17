use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::auth::{DeviceStart, PollOutcome};
use crate::db::{self, Account};
use crate::error::{Error, Result};
use crate::github;
use crate::keychain;
use crate::notify;
use crate::state::{AppState, Pending};
use crate::tray;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    db_path: String,
    schema_version: i64,
    api_base: String,
    web_base: String,
    provider: String,
    client_id_configured: bool,
}

#[tauri::command]
pub fn app_info(app: AppHandle, state: State<AppState>) -> Result<AppInfo> {
    let db = state.db.lock().unwrap();
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        db_path: db.path().unwrap_or("(memory)").to_string(),
        schema_version: db::schema_version(&db)?,
        api_base: state.host.api_base.clone(),
        web_base: state.host.web_base.clone(),
        provider: state
            .auth
            .as_ref()
            .map(|a| a.id().to_string())
            .unwrap_or_else(|| "none".into()),
        client_id_configured: state.auth.is_some(),
    })
}

/// An account row without a keychain entry is a half-signed-out state (the
/// user cleared Keychain Access, or a restore moved the database between
/// machines). Report it as signed out rather than showing an app that 401s.
#[tauri::command]
pub fn auth_status(state: State<AppState>) -> Result<Option<Account>> {
    let db = state.db.lock().unwrap();
    let Some(account) = db::current_account(&db)? else {
        return Ok(None);
    };
    if state.token_for(&account.host, &account.login)?.is_none() {
        return Ok(None);
    }
    Ok(Some(account))
}

#[tauri::command]
pub async fn auth_begin(app: AppHandle, state: State<'_, AppState>) -> Result<DeviceStart> {
    let auth = state.auth.clone().ok_or(Error::NoClientId)?;
    state.cancel_pending();

    let start = auth.begin().await?;

    let handle = tauri::async_runtime::spawn(poll_until_granted(
        app.clone(),
        auth,
        start.device_code.clone(),
        start.interval,
        start.expires_in,
    ));
    *state.pending.lock().unwrap() = Some(Pending { handle });

    Ok(start)
}

#[tauri::command]
pub fn auth_cancel(state: State<AppState>) {
    state.cancel_pending();
}

#[tauri::command]
pub fn auth_logout(app: AppHandle, state: State<AppState>) -> Result<()> {
    state.cancel_pending();
    let db = state.db.lock().unwrap();
    if let Some(account) = db::current_account(&db)? {
        keychain::delete(&account.host, &account.login)?;
        db::delete_account(&db, account.id)?;
    }
    state.forget_token();
    crate::sync::stop_loop(&app);
    Ok(())
}

#[tauri::command]
pub fn send_test_notification(app: AppHandle) {
    notify::send(&app, "Workbench", "Notifications work with the window closed.");
}

#[tauri::command]
pub fn hide_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> Result<()> {
    // Only ever an https URL we produced; refuse anything else so a compromised
    // page cannot use this as a generic launcher.
    if !url.starts_with("https://") {
        return Err(Error::msg("refusing to open a non-https URL"));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| Error::msg(e.to_string()))
}

/// Runs until GitHub grants, denies, or the code expires. Lives in Rust so
/// closing the window mid-approval does not break the flow.
async fn poll_until_granted(
    app: AppHandle,
    auth: Arc<dyn crate::auth::AuthProvider>,
    device_code: String,
    interval: u64,
    expires_in: u64,
) {
    let mut wait = Duration::from_secs(interval);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(expires_in);

    loop {
        tokio::time::sleep(wait).await;

        if tokio::time::Instant::now() >= deadline {
            let _ = app.emit("auth:failed", "That code expired. Try connecting again.");
            return;
        }

        match auth.poll_once(&device_code).await {
            Ok(PollOutcome::Pending) => {}
            Ok(PollOutcome::SlowDown) => wait += Duration::from_secs(5),
            Ok(PollOutcome::Denied) => {
                let _ = app.emit("auth:failed", "Access was denied on GitHub.");
                return;
            }
            Ok(PollOutcome::Expired) => {
                let _ = app.emit("auth:failed", "That code expired. Try connecting again.");
                return;
            }
            Ok(PollOutcome::Granted(token)) => {
                match finish_sign_in(&app, auth.id(), &token.access_token).await {
                    Ok(account) => {
                        let login = account.login.clone();
                        let _ = app.emit("auth:completed", account);
                        tray::show_main(&app);
                        notify::send(&app, "Workbench is connected", &format!("Signed in as @{login}"));
                        crate::sync::spawn_loop(&app);
                    }
                    Err(e) => {
                        let _ = app.emit("auth:failed", e.to_string());
                    }
                }
                return;
            }
            Err(e) => {
                let _ = app.emit("auth:failed", e.to_string());
                return;
            }
        }
    }
}

async fn finish_sign_in(app: &AppHandle, provider: &str, token: &str) -> Result<Account> {
    // Identify the token before storing it: a token we cannot read /user with
    // is not worth keeping.
    let (http, host) = {
        let state = app.state::<AppState>();
        (state.http.clone(), state.host.clone())
    };
    let viewer = github::viewer(&http, &host, token).await?;
    let host_label = host.label();

    keychain::store(&host_label, &viewer.login, token)?;

    let state = app.state::<AppState>();
    // Signing in already has the token in hand — never read it back.
    state.remember_token(&host_label, &viewer.login, token);
    let db = state.db.lock().unwrap();
    let account = db::upsert_account(
        &db,
        provider,
        &host_label,
        &viewer.login,
        viewer.name.as_deref(),
        viewer.avatar_url.as_deref(),
    )?;
    db::record_event(&db, account.id, "app", &host_label, "connected", None, None)?;
    Ok(account)
}

// ---------------------------------------------------------------------------
// The Desk
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeskView {
    needs_you: Vec<db::PullRequestRow>,
    yours: Vec<db::PullRequestRow>,
    watching: Vec<db::PullRequestRow>,
    last_synced_at: Option<i64>,
    /// Organisations this token can actually see. An org you work in that is
    /// missing here has not approved the app, and its pull requests will be
    /// absent from search with no error of any kind.
    visible_orgs: Vec<String>,
    /// Where to go and fix that.
    org_access_url: Option<String>,
}

/// Reads local state only — never the network. The window opens instantly and
/// works on a plane; syncing is a separate, explicit thing.
#[tauri::command]
pub fn desk(state: State<AppState>) -> Result<DeskView> {
    let db = state.db.lock().unwrap();
    let Some(account) = db::current_account(&db)? else {
        return Ok(DeskView {
            needs_you: vec![],
            yours: vec![],
            watching: vec![],
            last_synced_at: None,
            visible_orgs: vec![],
            org_access_url: None,
        });
    };

    let mut view = DeskView {
        needs_you: vec![],
        yours: vec![],
        watching: vec![],
        last_synced_at: db::get_sync_meta(&db, crate::sync::DESK_KEY)?,
        visible_orgs: db::setting_get(&db, crate::sync::VISIBLE_ORGS_KEY)?
            .filter(|s| !s.is_empty())
            .map(|s| s.split(',').map(str::to_string).collect())
            .unwrap_or_default(),
        org_access_url: crate::config::client_id().map(|id| {
            format!("{}/settings/connections/applications/{id}", state.host.web_base)
        }),
    };
    for pr in db::list_pull_requests(&db, account.id)? {
        match pr.lane.as_str() {
            "needs_you" => view.needs_you.push(pr),
            "yours" => view.yours.push(pr),
            _ => view.watching.push(pr),
        }
    }
    Ok(view)
}

#[tauri::command]
pub async fn sync_now(app: AppHandle) -> Result<crate::sync::SyncOutcome> {
    crate::sync::run_and_notify(&app).await
}

/// Opening a PR clears its "new since you looked" markers.
#[tauri::command]
pub fn open_pull_request(app: AppHandle, state: State<AppState>, id: String, url: String) -> Result<()> {
    {
        let db = state.db.lock().unwrap();
        db::mark_opened(&db, &id)?;
    }
    open_external(app, url)
}

// ---------------------------------------------------------------------------
// Daily notes
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteView {
    day: String,
    body: String,
    updated_at: i64,
    is_today: bool,
    previous_day: Option<String>,
    next_day: Option<String>,
    done: usize,
    open: usize,
}

fn view(
    db: &rusqlite::Connection,
    store: &crate::notes::Store,
    note: db::Note,
) -> Result<NoteView> {
    let (previous_day, next_day) = crate::notes::neighbours(db, store, &note.day)?;
    let (done, open) = crate::notes::tally(&note.body);
    Ok(NoteView {
        is_today: note.day == crate::notes::today(),
        previous_day,
        next_day,
        done,
        open,
        day: note.day,
        body: note.body,
        updated_at: note.updated_at,
    })
}

/// `day` is None for today. Creating happens here, lazily — there is no
/// "new note" button anywhere in this app, by design.
#[tauri::command]
pub fn note_open(state: State<AppState>, day: Option<String>) -> Result<NoteView> {
    let db = state.db.lock().unwrap();
    let store = crate::notes::store_for(&db)?;
    let day = day.unwrap_or_else(crate::notes::today);
    let note = crate::notes::open_day(&db, &store, &day)?;
    view(&db, &store, note)
}

#[tauri::command]
pub fn note_save(state: State<AppState>, day: String, body: String) -> Result<NoteView> {
    let db = state.db.lock().unwrap();
    let store = crate::notes::store_for(&db)?;
    let note = crate::notes::save(&db, &store, &day, &body)?;
    view(&db, &store, note)
}

#[tauri::command]
pub fn note_search(state: State<AppState>, query: String) -> Result<Vec<db::Note>> {
    let db = state.db.lock().unwrap();
    if query.trim().is_empty() {
        return Ok(vec![]);
    }
    db::note_search(&db, query.trim())
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Where notes are written. `None` means the app's own database.
    notes_dir: Option<String>,
    /// Shown so it is obvious what "app storage" actually means on disk.
    db_path: String,
    version: String,
    schema_version: i64,
    api_base: String,
    account: Option<Account>,
}

#[tauri::command]
pub fn settings_read(app: AppHandle, state: State<AppState>) -> Result<Settings> {
    let db = state.db.lock().unwrap();
    Ok(Settings {
        notes_dir: db::setting_get(&db, crate::notes::NOTES_DIR_KEY)?,
        db_path: db.path().unwrap_or("(memory)").to_string(),
        version: app.package_info().version.to_string(),
        schema_version: db::schema_version(&db)?,
        api_base: state.host.api_base.clone(),
        account: db::current_account(&db)?,
    })
}

/// Hands note storage over to a folder, copying across what already exists.
/// The path comes from the OS folder picker, so the webview cannot invent one.
#[tauri::command]
pub fn settings_set_notes_dir(state: State<AppState>, path: String) -> Result<usize> {
    let dir = std::path::PathBuf::from(path);
    if !dir.is_dir() {
        return Err(Error::msg("that is not a folder"));
    }
    let db = state.db.lock().unwrap();
    crate::notes::adopt_folder(&db, &dir)
}

#[tauri::command]
pub fn settings_clear_notes_dir(state: State<AppState>) -> Result<()> {
    let db = state.db.lock().unwrap();
    crate::notes::release_folder(&db)
}

/// Reveals the notes folder (or the database) in the file manager.
#[tauri::command]
pub fn settings_reveal_notes(app: AppHandle, state: State<AppState>) -> Result<()> {
    let target = {
        let db = state.db.lock().unwrap();
        match db::setting_get(&db, crate::notes::NOTES_DIR_KEY)? {
            Some(dir) => dir,
            None => db
                .path()
                .and_then(|p| std::path::Path::new(p).parent())
                .map(|p| p.to_string_lossy().to_string())
                .ok_or_else(|| Error::msg("no folder to open"))?,
        }
    };
    app.opener()
        .open_path(target, None::<&str>)
        .map_err(|e| Error::msg(e.to_string()))
}
