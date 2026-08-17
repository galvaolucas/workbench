//! Notifications are sent from Rust, never from the webview.
//!
//! The window is closed most of the day — that is the normal state for this
//! app, not an edge case — so anything that can only fire from a live page is
//! useless. On macOS this also means notifications only appear from a bundled,
//! signed app: `npm run app` (raw binary) will not show them, `npm run app:build`
//! will. That is a macOS rule, not a Tauri bug.

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_notification::NotificationExt;

use crate::db::{self, PendingEvent};
use crate::error::Result;
use crate::state::AppState;

pub fn send<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("notification failed: {e}");
    }
}

/// Only four things are worth interrupting you for. Everything else is recorded
/// and shown as a marker on the card, but stays silent.
fn worth_telling(e: &PendingEvent) -> Option<(u8, String, String)> {
    let subject = format!("{}#{} — {}", e.repo, e.number, e.title);

    match e.kind.as_str() {
        "review_requested" => Some((
            0,
            "Review requested".into(),
            match &e.author {
                Some(a) => format!("@{a} — {subject}"),
                None => subject,
            },
        )),
        "checks" if e.is_author && matches!(e.to_state.as_deref(), Some("FAILURE" | "ERROR")) => {
            Some((1, "Checks failed".into(), format!("Your PR {subject}")))
        }
        "review_decision" if e.is_author => match e.to_state.as_deref() {
            Some("CHANGES_REQUESTED") => {
                Some((2, "Changes requested".into(), format!("Your PR {subject}")))
            }
            Some("APPROVED") => Some((3, "Approved".into(), format!("Your PR {subject}"))),
            _ => None,
        },
        "new_comments" if e.is_author || e.is_reviewer => {
            Some((4, "New comments".into(), subject))
        }
        // new_commits and closed are noise as notifications; the card shows them.
        _ => None,
    }
}

/// Turns the outstanding event log into at most a handful of notifications,
/// then marks the whole backlog handled.
///
/// `send_them` is false when you are already looking at the app — the Desk's
/// own markers say the same thing without a banner over your editor.
pub fn flush(app: &AppHandle, send_them: bool) -> Result<usize> {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();

    let Some(account) = db::current_account(&db)? else {
        return Ok(0);
    };

    let pending = db::pending_notifications(&db, account.id)?;
    db::mark_all_notified(&db, account.id)?;
    drop(db);

    if !send_them || pending.is_empty() {
        return Ok(0);
    }

    // One notification per pull request, not per event: twelve check updates on
    // one PR is one thing happening, not twelve.
    let mut best: Vec<(String, u8, String, String)> = Vec::new();
    for event in &pending {
        let Some((rank, title, body)) = worth_telling(event) else {
            continue;
        };
        match best.iter_mut().find(|(id, ..)| *id == event.pr_id) {
            Some(existing) if rank < existing.1 => {
                *existing = (event.pr_id.clone(), rank, title, body);
            }
            Some(_) => {}
            None => best.push((event.pr_id.clone(), rank, title, body)),
        }
    }

    if best.is_empty() {
        return Ok(0);
    }

    // And beyond a few, one summary — a stack of banners is worse than a count.
    if best.len() > 3 {
        send(
            app,
            "Workbench",
            &format!("{} pull requests changed while you were away", best.len()),
        );
        return Ok(1);
    }

    best.sort_by_key(|(_, rank, ..)| *rank);
    for (_, _, title, body) in &best {
        send(app, title, body);
    }
    Ok(best.len())
}
