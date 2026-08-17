use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),

    #[error("GitHub said: {0}")]
    GitHub(String),

    #[error("no GitHub client ID configured")]
    NoClientId,

    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("keychain error: {0}")]
    Keychain(#[from] keyring::Error),

    #[error("{0}")]
    Tauri(#[from] tauri::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    pub fn msg(m: impl Into<String>) -> Self {
        Error::Message(m.into())
    }
}

/// Commands return this straight to the webview, so it must be a plain string —
/// never a structured error that could leak a token or a device code.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
