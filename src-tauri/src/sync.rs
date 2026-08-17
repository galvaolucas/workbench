//! One sync: fetch the Desk, diff it against what we already knew, write both
//! the new state and the transitions, in a single transaction.
//!
//! Nothing here decides what to *show* — lanes are computed at read time — and
//! nothing here sends a notification. It only records what changed. M2's
//! notifier reads the events this leaves behind.

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::db::{self, PrInput};
use crate::error::{Error, Result};
use crate::github;
use crate::state::AppState;

pub const DESK_KEY: &str = "desk";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOutcome {
    pub pull_requests: usize,
    pub events: usize,
    pub retired: usize,
    pub cost: i64,
    pub remaining: i64,
    pub synced_at: i64,
}

pub async fn run(app: &AppHandle) -> Result<SyncOutcome> {
    let (http, host, account, token) = {
        let state = app.state::<AppState>();
        let db = state.db.lock().unwrap();
        let account =
            db::current_account(&db)?.ok_or_else(|| Error::msg("not signed in"))?;
        let token = state
            .token_for(&account.host, &account.login)?
            .ok_or_else(|| {
                Error::msg("no token in the keychain — sign out and reconnect your account")
            })?;
        (state.http.clone(), state.host.clone(), account, token)
    };

    let data = github::desk(&http, &host, &token, &account.login).await?;
    let (cost, remaining) = data
        .rate_limit
        .as_ref()
        .map(|r| (r.cost, r.remaining))
        .unwrap_or((0, 0));

    // A PR can arrive in several result sets at once — yours *and* mentioned,
    // say. Merge on id and OR the relations together.
    let mut merged: HashMap<String, PrInput> = HashMap::new();
    collect(&mut merged, data.mine.nodes, true, false, false);
    collect(&mut merged, data.reviewing.nodes, false, true, false);
    collect(&mut merged, data.mentioned.nodes, false, false, true);
    collect(&mut merged, data.involved.nodes, false, false, false);

    let state = app.state::<AppState>();
    let mut guard = state.db.lock().unwrap();
    let conn = &mut *guard;

    // Connecting an account should not fire a notification for every review
    // already waiting on you. The first sync records everything, silently.
    let first_sync = db::get_sync_meta(conn, DESK_KEY)?.is_none();

    let synced_at = db::now_secs();
    let tx = conn.transaction()?;
    let mut events = 0;
    for pr in merged.values() {
        events += db::upsert_pull_request(&tx, account.id, pr)?;
    }
    let retired = db::retire_missing(&tx, account.id, synced_at)?;
    db::set_sync_meta(&tx, DESK_KEY, synced_at)?;
    if first_sync {
        db::mark_all_notified(&tx, account.id)?;
    }
    let waiting = db::needs_you_count(&tx, account.id)?;
    tx.commit()?;
    drop(guard);

    crate::tray::set_badge(app, waiting);

    let outcome = SyncOutcome {
        pull_requests: merged.len(),
        events,
        retired,
        cost,
        remaining,
        synced_at,
    };

    log::info!(
        "sync: {} PRs, {} events, {} retired, cost {} ({} left)",
        outcome.pull_requests,
        outcome.events,
        outcome.retired,
        cost,
        remaining
    );
    let _ = app.emit("desk:updated", outcome.clone());

    Ok(outcome)
}

fn collect(
    map: &mut HashMap<String, PrInput>,
    prs: Vec<github::PullRequest>,
    is_author: bool,
    is_reviewer: bool,
    is_mentioned: bool,
) {
    for pr in prs {
        match map.entry(pr.id.clone()) {
            Entry::Occupied(mut e) => {
                let v = e.get_mut();
                v.is_author |= is_author;
                v.is_reviewer |= is_reviewer;
                v.is_mentioned |= is_mentioned;
            }
            Entry::Vacant(e) => {
                e.insert(to_input(&pr, is_author, is_reviewer, is_mentioned));
            }
        }
    }
}

fn to_input(
    pr: &github::PullRequest,
    is_author: bool,
    is_reviewer: bool,
    is_mentioned: bool,
) -> PrInput {
    PrInput {
        id: pr.id.clone(),
        repo: pr.repository.name_with_owner.clone(),
        number: pr.number,
        title: pr.title.clone(),
        url: pr.url.clone(),
        author: pr.author.as_ref().map(|a| a.login.clone()),
        author_avatar: pr.author.as_ref().and_then(|a| a.avatar_url.clone()),
        is_draft: pr.is_draft,
        additions: pr.additions,
        deletions: pr.deletions,
        changed_files: pr.changed_files,
        comment_count: pr.comments.total_count,
        review_decision: pr.review_decision.clone(),
        checks_state: pr.checks_state().map(str::to_string),
        head_oid: pr.head_oid().map(str::to_string),
        is_author,
        is_reviewer,
        is_mentioned,
        created_at: epoch(&pr.created_at),
        updated_at: epoch(&pr.updated_at),
    }
}

fn epoch(iso: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|d| d.timestamp())
        .unwrap_or(0)
}

/// Runs a sync, then turns whatever it recorded into notifications.
///
/// Silent while you are looking at the app: the Desk's own markers already say
/// what changed, and a banner over your editor would be telling you twice.
pub async fn run_and_notify(app: &AppHandle) -> Result<SyncOutcome> {
    let outcome = run(app).await?;
    let announce = !app.state::<AppState>().is_focused();
    if let Err(e) = crate::notify::flush(app, announce) {
        log::warn!("could not flush notifications: {e}");
    }
    Ok(outcome)
}

/// The background poller. Replaces any loop already running, so signing in and
/// out never leaves two of them racing.
pub fn spawn_loop(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut slot = state.sync_task.lock().unwrap();
    if let Some(existing) = slot.take() {
        existing.abort();
    }

    let app = app.clone();
    *slot = Some(tauri::async_runtime::spawn(async move {
        loop {
            // Re-read the interval every tick: the cadence follows where your
            // attention is, and that changes while we sleep.
            let wait = app.state::<AppState>().poll_interval();
            tokio::time::sleep(wait).await;

            match run_and_notify(&app).await {
                Ok(o) => log::debug!("background sync: {} PRs", o.pull_requests),
                // A failed poll is normal — closed laptop, dropped wifi, GitHub
                // hiccup. Log it and let the next tick try again.
                Err(e) => log::warn!("background sync failed: {e}"),
            }
        }
    }));
}

/// Backs off to the slow tier immediately after a failure would be wrong —
/// but a hard stop is right when there is nothing to sync.
pub fn stop_loop(app: &AppHandle) {
    if let Some(handle) = app.state::<AppState>().sync_task.lock().unwrap().take() {
        handle.abort();
    }
    crate::tray::set_badge(app, 0);
}
