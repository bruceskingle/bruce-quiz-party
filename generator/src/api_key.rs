//! The user's own key for Claude's API, kept in the system's credential store, the Keychain on macOS, never in a file,
//! so that it is not in a backup or a copy of the settings. A sandboxed app, e.g. the Mac App Store's copy, keeps its
//! own items in the Keychain too.

use anyhow::anyhow;

/// The Keychain item's service and account, which name it in Keychain Access.
const SERVICE: &str = "org.skingle.bruce-quiz-party";
const ACCOUNT: &str = "anthropic-api-key";

fn entry() -> anyhow::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|error| anyhow!("cannot use the Keychain: {error}"))
}

/// The key saved, or None if there is none.
pub fn load() -> anyhow::Result<Option<String>> {
    match entry()?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(anyhow!("cannot read the API key from the Keychain: {error}")),
    }
}

/// Saves a key, replacing any saved before.
pub fn save(key: &str) -> anyhow::Result<()> {
    let key = key.trim();
    anyhow::ensure!(!key.is_empty(), "the API key is empty");
    entry()?
        .set_password(key)
        .map_err(|error| anyhow!("cannot save the API key in the Keychain: {error}"))
}

/// Forgets the key saved, if there is one.
pub fn forget() -> anyhow::Result<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(anyhow!("cannot remove the API key from the Keychain: {error}")),
    }
}
