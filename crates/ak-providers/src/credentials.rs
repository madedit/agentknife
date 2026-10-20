//! API keys: kept only in the OS keychain, held in memory as [`ApiKey`].
//!
//! `ApiKey` cannot be serialized and prints as `<redacted>`, so a key cannot
//! end up in a settings file, a fixture or a log by accident.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use ak_types::settings::AiConnection;
use thiserror::Error;

/// Keychain service name for every AgentKnife key.
pub const SERVICE: &str = "AgentKnife";

/// An API key in memory. No `Serialize`, no `Display`; `Debug` is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// Trims whitespace (pasted keys often carry a newline); `None` if empty.
    pub fn new(key: &str) -> Option<ApiKey> {
        let key = key.trim();
        (!key.is_empty()).then(|| ApiKey(key.to_owned()))
    }

    /// The key itself, for the request header. Never log or store the result.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// `text` with every copy of the key replaced (servers sometimes echo it).
    pub fn redact(&self, text: &str) -> String {
        if self.0.is_empty() {
            text.to_owned()
        } else {
            text.replace(&self.0, "<redacted>")
        }
    }
}

impl From<String> for ApiKey {
    fn from(key: String) -> Self {
        ApiKey(key)
    }
}

impl From<&str> for ApiKey {
    fn from(key: &str) -> Self {
        ApiKey(key.to_owned())
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// Keychain failures. Messages never contain the key (keyring's `BadEncoding`
/// carries the raw secret bytes, so its details are dropped).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CredentialError {
    /// No keychain on this system, or it is locked / access was denied.
    #[error("the system keychain is not available: {0}")]
    Unavailable(String),
    #[error("keychain error: {0}")]
    Failed(String),
}

/// Where keys live. [`Keychain`] in the app; [`MemoryStore`] in tests.
pub trait CredentialStore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<ApiKey>, CredentialError>;
    fn set(&self, account: &str, key: &ApiKey) -> Result<(), CredentialError>;
    /// Removing a key that is not there is not an error.
    fn delete(&self, account: &str) -> Result<(), CredentialError>;
}

/// The keychain account for a connection: the provider id, plus the endpoint
/// when it is not the default (each local or custom server has its own key).
pub fn account(conn: &AiConnection) -> String {
    match &conn.base_url {
        None => conn.provider.clone(),
        Some(url) => format!(
            "{}@{}",
            conn.provider,
            url.trim().trim_end_matches('/').to_ascii_lowercase()
        ),
    }
}

/// The OS keychain: Windows Credential Manager, macOS Keychain, Secret Service.
#[derive(Debug, Default, Clone, Copy)]
pub struct Keychain;

impl Keychain {
    fn entry(account: &str) -> Result<keyring::Entry, CredentialError> {
        keyring::Entry::new(SERVICE, account).map_err(map_error)
    }
}

fn map_error(e: keyring::Error) -> CredentialError {
    use keyring::Error as E;
    match e {
        E::NoStorageAccess(p) => CredentialError::Unavailable(p.to_string()),
        E::NoDefaultStore => CredentialError::Unavailable("no credential store".into()),
        E::PlatformFailure(p) => CredentialError::Failed(p.to_string()),
        // These carry the stored bytes: never show them.
        E::BadEncoding(_) | E::BadDataFormat(..) => {
            CredentialError::Failed("the stored key is unreadable".into())
        }
        E::NoEntry => CredentialError::Failed("no entry".into()),
        E::Ambiguous(_) => CredentialError::Failed("more than one matching entry".into()),
        E::TooLong(attr, limit) => CredentialError::Failed(format!("{attr} longer than {limit}")),
        E::Invalid(attr, why) => CredentialError::Failed(format!("{attr}: {why}")),
        _ => CredentialError::Failed("unknown keychain error".into()),
    }
}

impl CredentialStore for Keychain {
    fn get(&self, account: &str) -> Result<Option<ApiKey>, CredentialError> {
        match Self::entry(account)?.get_password() {
            Ok(key) => Ok(ApiKey::new(&key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_error(e)),
        }
    }

    fn set(&self, account: &str, key: &ApiKey) -> Result<(), CredentialError> {
        Self::entry(account)?
            .set_password(key.expose())
            .map_err(map_error)
    }

    fn delete(&self, account: &str) -> Result<(), CredentialError> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_error(e)),
        }
    }
}

/// Keys in memory only (tests, and "use once without saving").
#[derive(Debug, Default)]
pub struct MemoryStore {
    keys: Mutex<HashMap<String, ApiKey>>,
}

impl CredentialStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<ApiKey>, CredentialError> {
        let keys = self.keys.lock().map_err(|_| poisoned())?;
        Ok(keys.get(account).cloned())
    }

    fn set(&self, account: &str, key: &ApiKey) -> Result<(), CredentialError> {
        let mut keys = self.keys.lock().map_err(|_| poisoned())?;
        keys.insert(account.to_owned(), key.clone());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), CredentialError> {
        let mut keys = self.keys.lock().map_err(|_| poisoned())?;
        keys.remove(account);
        Ok(())
    }
}

fn poisoned() -> CredentialError {
    CredentialError::Failed("store lock poisoned".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(provider: &str, base_url: Option<&str>) -> AiConnection {
        AiConnection {
            provider: provider.into(),
            base_url: base_url.map(Into::into),
            model: "m".into(),
            effort: None,
        }
    }

    #[test]
    fn accounts_separate_providers_and_endpoints() {
        assert_eq!(account(&conn("anthropic", None)), "anthropic");
        assert_eq!(
            account(&conn(
                "openai-compatible",
                Some("http://127.0.0.1:8080/v1/")
            )),
            "openai-compatible@http://127.0.0.1:8080/v1"
        );
        assert_ne!(
            account(&conn("anthropic", Some("http://127.0.0.1:8080"))),
            account(&conn("anthropic", None)),
            "a local Anthropic-compatible server does not get the real key"
        );
    }

    #[test]
    fn key_is_trimmed_redacted_and_scrubbed() {
        assert_eq!(ApiKey::new("  \n"), None);
        let key = ApiKey::new(" sk-ant-123456\n").unwrap();
        assert_eq!(key.expose(), "sk-ant-123456");
        assert_eq!(format!("{key:?}"), "ApiKey(<redacted>)");
        assert_eq!(
            key.redact("Invalid API key: sk-ant-123456."),
            "Invalid API key: <redacted>."
        );
    }

    #[test]
    fn memory_store_round_trip() {
        let store = MemoryStore::default();
        let key = ApiKey::from("sk-1");
        store.set("anthropic", &key).unwrap();
        assert_eq!(store.get("anthropic").unwrap(), Some(key));
        store.delete("anthropic").unwrap();
        store.delete("anthropic").unwrap();
        assert_eq!(store.get("anthropic").unwrap(), None);
    }
}
