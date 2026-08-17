use reqwest::Client;
use serde::Deserialize;

use super::{AuthProvider, AuthToken, DeviceStart, PollOutcome};
use crate::config::{Host, SCOPES};
use crate::error::{Error, Result};

pub struct DeviceFlow {
    client_id: String,
    host: Host,
    http: Client,
}

impl DeviceFlow {
    pub fn new(client_id: String, host: Host, http: Client) -> Self {
        Self {
            client_id,
            host,
            http,
        }
    }
}

#[derive(Deserialize)]
struct StartResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

/// GitHub returns 200 for both success and failure here; the shape tells you which.
#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

#[async_trait::async_trait]
impl AuthProvider for DeviceFlow {
    fn id(&self) -> &'static str {
        "github-oauth-device"
    }

    async fn begin(&self) -> Result<DeviceStart> {
        let res = self
            .http
            .post(self.host.device_code_url())
            .header("Accept", "application/json")
            .form(&[("client_id", self.client_id.as_str()), ("scope", SCOPES)])
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(Error::GitHub(format!("{status} starting device flow — {body}")));
        }

        let start: StartResponse = res.json().await?;
        Ok(DeviceStart {
            user_code: start.user_code,
            verification_uri: start.verification_uri,
            expires_in: start.expires_in,
            device_code: start.device_code,
            // Never poll faster than GitHub asks, or it starts returning slow_down.
            interval: start.interval.max(5),
        })
    }

    async fn poll_once(&self, device_code: &str) -> Result<PollOutcome> {
        let res: TokenResponse = self
            .http
            .post(self.host.access_token_url())
            .header("Accept", "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("device_code", device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await?
            .json()
            .await?;

        if let Some(token) = res.access_token {
            return Ok(PollOutcome::Granted(Box::new(AuthToken {
                access_token: token,
                refresh_token: res.refresh_token,
                expires_at: res.expires_in.map(|s| crate::db::now_secs() + s),
            })));
        }

        match res.error.as_deref() {
            Some("authorization_pending") => Ok(PollOutcome::Pending),
            Some("slow_down") => Ok(PollOutcome::SlowDown),
            Some("expired_token") => Ok(PollOutcome::Expired),
            Some("access_denied") => Ok(PollOutcome::Denied),
            Some(other) => Err(Error::GitHub(
                res.error_description.unwrap_or_else(|| other.to_string()),
            )),
            None => Err(Error::GitHub("empty response from token endpoint".into())),
        }
    }
}
