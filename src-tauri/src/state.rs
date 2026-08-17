use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::Client;
use rusqlite::Connection;
use tauri::async_runtime::JoinHandle;

use crate::auth::AuthProvider;
use crate::config::Host;
use crate::error::Result;
use crate::keychain;

/// A device-flow grant we are waiting on. Kept in Rust so the pending
/// `device_code` never reaches the webview.
pub struct Pending {
    pub handle: JoinHandle<()>,
}

pub struct CachedToken {
    host: String,
    login: String,
    secret: String,
}

pub struct AppState {
    pub db: Mutex<Connection>,
    pub http: Client,
    pub host: Host,
    /// `None` until a client ID is configured — the UI explains how.
    pub auth: Option<Arc<dyn AuthProvider>>,
    pub pending: Mutex<Option<Pending>>,
    /// The access token, held in memory for the life of the process.
    token: Mutex<Option<CachedToken>>,
    /// Is the window in front right now?
    focused: AtomicBool,
    /// When it last was — how we tell "in the background" from "gone for lunch".
    last_focus_at: AtomicI64,
    pub sync_task: Mutex<Option<JoinHandle<()>>>,
}

impl AppState {
    pub fn new(
        db: Connection,
        http: Client,
        host: Host,
        auth: Option<Arc<dyn AuthProvider>>,
    ) -> Self {
        Self {
            db: Mutex::new(db),
            http,
            host,
            auth,
            pending: Mutex::new(None),
            token: Mutex::new(None),
            focused: AtomicBool::new(true),
            last_focus_at: AtomicI64::new(crate::db::now_secs()),
            sync_task: Mutex::new(None),
        }
    }

    pub fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::Relaxed);
        if !focused {
            self.last_focus_at
                .store(crate::db::now_secs(), Ordering::Relaxed);
        }
    }

    pub fn is_focused(&self) -> bool {
        self.focused.load(Ordering::Relaxed)
    }

    /// Poll fast enough that the Desk is current when you look at it, slow
    /// enough to be invisible in the rate limit. A sync costs 2 points of
    /// 5,000/hour, so even the fastest tier here spends about 2%.
    pub fn poll_interval(&self) -> Duration {
        if self.is_focused() {
            return Duration::from_secs(60);
        }
        let away = crate::db::now_secs() - self.last_focus_at.load(Ordering::Relaxed);
        if away > 15 * 60 {
            Duration::from_secs(15 * 60)
        } else {
            Duration::from_secs(5 * 60)
        }
    }

    pub fn cancel_pending(&self) {
        if let Some(p) = self.pending.lock().unwrap().take() {
            p.handle.abort();
        }
    }

    /// At most one keychain read per launch.
    ///
    /// macOS binds keychain access rights to an app's code signature, so an
    /// unsigned dev build is treated as a different app on every rebuild and
    /// prompts on every read — "Always Allow" cannot stick. Reading once and
    /// holding the token in memory is also simply correct: the keychain
    /// protects secrets at rest, and the token is already in this process's
    /// memory whenever it is used. The webview still never sees it.
    pub fn token_for(&self, host: &str, login: &str) -> Result<Option<String>> {
        if let Some(cached) = self.token.lock().unwrap().as_ref() {
            if cached.host == host && cached.login == login {
                return Ok(Some(cached.secret.clone()));
            }
        }

        let Some(secret) = keychain::read(host, login)? else {
            return Ok(None);
        };
        self.remember_token(host, login, &secret);
        Ok(Some(secret))
    }

    /// Called on sign-in, so a fresh connection never reads the keychain back.
    pub fn remember_token(&self, host: &str, login: &str, secret: &str) {
        *self.token.lock().unwrap() = Some(CachedToken {
            host: host.to_string(),
            login: login.to_string(),
            secret: secret.to_string(),
        });
    }

    pub fn forget_token(&self) {
        *self.token.lock().unwrap() = None;
    }
}
