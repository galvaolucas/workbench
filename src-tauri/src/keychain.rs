//! Token storage. The webview has no command that returns a token, and no
//! GitHub call is ever made outside Rust — so a compromised page cannot
//! exfiltrate credentials.

use keyring::Entry;

use crate::config::KEYCHAIN_SERVICE;
use crate::error::Result;

/// One entry per account, keyed by host so github.com and an Enterprise
/// account can coexist.
fn key(host: &str, login: &str) -> String {
    format!("{host}/{login}")
}

pub fn store(host: &str, login: &str, token: &str) -> Result<()> {
    Entry::new(KEYCHAIN_SERVICE, &key(host, login))?.set_password(token)?;
    Ok(())
}

pub fn read(host: &str, login: &str) -> Result<Option<String>> {
    match Entry::new(KEYCHAIN_SERVICE, &key(host, login))?.get_password() {
        Ok(t) => Ok(Some(t)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn delete(host: &str, login: &str) -> Result<()> {
    match Entry::new(KEYCHAIN_SERVICE, &key(host, login))?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
