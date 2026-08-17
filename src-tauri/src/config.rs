use serde::{Deserialize, Serialize};

/// Keychain service name. Changing this orphans every stored token.
pub const KEYCHAIN_SERVICE: &str = "com.galvaolucas.workbench";

pub const USER_AGENT: &str = concat!("workbench/", env!("CARGO_PKG_VERSION"));

/// `repo` is the expensive one: it is all-or-nothing and security-minded orgs
/// will refuse it. That is the reason [`crate::auth::AuthProvider`] exists —
/// a GitHub App provider can replace this without touching anything else.
pub const SCOPES: &str = "repo read:org notifications read:user";

/// Where a GitHub lives. Enterprise Server is a different base URL and nothing
/// else, so paying for it now costs one struct and saves a refactor later.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    /// e.g. `https://api.github.com` or `https://ghe.corp/api/v3`
    pub api_base: String,
    /// e.g. `https://github.com` or `https://ghe.corp`
    pub web_base: String,
}

impl Default for Host {
    fn default() -> Self {
        Host::dotcom()
    }
}

impl Host {
    pub fn dotcom() -> Self {
        Self {
            api_base: "https://api.github.com".into(),
            web_base: "https://github.com".into(),
        }
    }

    /// GitHub Enterprise Server: `https://ghe.corp` -> API at `/api/v3`.
    pub fn enterprise(web_base: &str) -> Self {
        let web = web_base.trim_end_matches('/').to_string();
        Self {
            api_base: format!("{web}/api/v3"),
            web_base: web,
        }
    }

    /// `WORKBENCH_GITHUB_HOST=https://ghe.corp` switches the whole app over.
    pub fn from_env() -> Self {
        match std::env::var("WORKBENCH_GITHUB_HOST") {
            Ok(v) if !v.trim().is_empty() => Host::enterprise(v.trim()),
            _ => Host::dotcom(),
        }
    }

    /// Short name for display and for keychain keys.
    pub fn label(&self) -> String {
        self.web_base
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string()
    }

    pub fn device_code_url(&self) -> String {
        format!("{}/login/device/code", self.web_base)
    }

    pub fn access_token_url(&self) -> String {
        format!("{}/login/oauth/access_token", self.web_base)
    }

    pub fn user_url(&self) -> String {
        format!("{}/user", self.api_base)
    }

    #[allow(dead_code)] // M1 uses this for the composed PR query.
    pub fn graphql_url(&self) -> String {
        if self.api_base.ends_with("/api/v3") {
            format!("{}/api/graphql", self.web_base)
        } else {
            format!("{}/graphql", self.api_base)
        }
    }
}

/// Read at runtime so you can test without rebuilding, baked in at compile time
/// for a distributed binary. A device-flow client ID is not a secret.
pub fn client_id() -> Option<String> {
    if let Ok(v) = std::env::var("WORKBENCH_GITHUB_CLIENT_ID") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return Some(v);
        }
    }
    option_env!("WORKBENCH_GITHUB_CLIENT_ID")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}
