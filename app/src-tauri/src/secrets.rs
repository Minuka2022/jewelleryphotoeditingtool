//! The Gemini API key, kept in the Windows Credential Manager so it is never
//! written to a file.

use keyring::Entry;

const SERVICE: &str = "Jewelry Studio";
const ACCOUNT: &str = "gemini-api-key";

fn entry() -> Result<Entry, String> {
    Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| format!("Cannot reach the Windows Credential Manager: {e}"))
}

/// The saved key. Falls back to the `GEMINI_API_KEY` environment variable,
/// which is how developers and tests supply one.
pub fn load() -> Option<String> {
    let saved = entry().ok().and_then(|entry| entry.get_password().ok());
    saved
        .or_else(|| std::env::var("GEMINI_API_KEY").ok())
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
}

/// Saves the key, or removes the saved key when `key` is blank.
pub fn save(key: &str) -> Result<(), String> {
    let key = key.trim();
    let entry = entry()?;
    if key.is_empty() {
        return match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(format!("Cannot remove the saved key: {error}")),
        };
    }
    entry
        .set_password(key)
        .map_err(|error| format!("Cannot save the key: {error}"))
}
