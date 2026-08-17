//! Authentication behind a trait.
//!
//! v1 ships the OAuth device flow because it needs no client secret, which is
//! the only sane option for a binary you hand to strangers. The catch is the
//! `repo` scope: all-or-nothing, and companies will push back. The answer is a
//! GitHub App (per-org install, fine-grained permissions, device flow works
//! there too) — so the boundary lives here from day one. Swapping providers
//! after you have users means re-onboarding all of them.

pub mod device_flow;

use serde::Serialize;

use crate::error::Result;

/// What the user needs to see to approve the app. `device_code` never crosses
/// into the webview — it is a bearer of the pending grant.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStart {
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,

    #[serde(skip)]
    pub device_code: String,
    #[serde(skip)]
    pub interval: u64,
}

#[derive(Clone, Debug)]
pub struct AuthToken {
    pub access_token: String,
    /// GitHub Apps issue expiring user tokens; OAuth apps do not.
    #[allow(dead_code)]
    pub refresh_token: Option<String>,
    #[allow(dead_code)]
    pub expires_at: Option<i64>,
}

#[derive(Debug)]
pub enum PollOutcome {
    Pending,
    /// GitHub asks us to back off; the caller widens its interval.
    SlowDown,
    Granted(Box<AuthToken>),
    Denied,
    Expired,
}

#[async_trait::async_trait]
pub trait AuthProvider: Send + Sync {
    /// Stable id recorded on the account row, so a future migration can tell
    /// an OAuth account from a GitHub App one.
    fn id(&self) -> &'static str;

    async fn begin(&self) -> Result<DeviceStart>;

    async fn poll_once(&self, device_code: &str) -> Result<PollOutcome>;
}
