//! Finds the Gemini API key: the `GEMINI_API_KEY` environment variable
//! first, then a `.env` file.

use std::fs;
use std::path::Path;

pub const KEY_NAME: &str = "GEMINI_API_KEY";

pub fn load_api_key(dotenv: &Path) -> Option<String> {
    let from_environment = std::env::var(KEY_NAME).ok();
    if let Some(key) = from_environment.as_deref().map(str::trim) {
        if !key.is_empty() {
            return Some(key.to_owned());
        }
    }
    parse_dotenv(&fs::read_to_string(dotenv).ok()?)
}

fn parse_dotenv(text: &str) -> Option<String> {
    // Notepad can save a byte-order mark at the start of the file.
    let text = text.trim_start_matches('\u{feff}');
    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() != KEY_NAME {
            continue;
        }
        let value = value.trim().trim_matches(['"', '\'']).trim();
        if !value.is_empty() {
            return Some(value.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_key_from_dotenv_text() {
        let text = "\u{feff}# comment\nOTHER=1\nGEMINI_API_KEY = \"abc123\" \n";
        assert_eq!(parse_dotenv(text).as_deref(), Some("abc123"));
    }

    #[test]
    fn an_empty_or_missing_key_is_none() {
        assert_eq!(parse_dotenv("GEMINI_API_KEY=\n"), None);
        assert_eq!(parse_dotenv("# GEMINI_API_KEY=abc\n"), None);
        assert_eq!(parse_dotenv(""), None);
    }
}
