// Copyright (C) 2026 Andrea Bozzo
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Persistent session-encryption key bootstrap.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;

use base64::Engine as _;
use tower_sessions::cookie::Key;

pub const DEFAULT_SESSION_KEY_FILE: &str = "data/session.key";

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn non_empty_env(name: &str) -> Result<Option<String>, String> {
    match std::env::var(name) {
        Ok(value) => Ok(non_empty(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} contains non-Unicode data")),
    }
}

fn decode(value: &str) -> Result<Key, String> {
    let cleaned: String = value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    let raw = base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .map_err(|error| format!("session encryption key is not valid base64: {error}"))?;
    Key::try_from(raw.as_slice()).map_err(|error| {
        format!("session encryption key must decode to at least 64 bytes: {error}")
    })
}

fn read_key(path: &Path) -> Result<Key, String> {
    let encoded = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    decode(&encoded)
}

fn create_key(path: &Path) -> Result<Key, String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }

    let key = Key::generate();
    let encoded = base64::engine::general_purpose::STANDARD.encode(key.master());
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            if let Err(error) = file
                .write_all(encoded.as_bytes())
                .and_then(|_| file.write_all(b"\n"))
                .and_then(|_| file.sync_all())
            {
                drop(file);
                let _ = fs::remove_file(path);
                return Err(format!("failed to write {}: {error}", path.display()));
            }
            Ok(key)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => read_key(path),
        Err(error) => Err(format!("failed to create {}: {error}", path.display())),
    }
}

pub fn load() -> Result<Key, String> {
    if let Some(value) = non_empty_env("SESSION_ENCRYPTION_KEY")? {
        return decode(&value);
    }

    let path = non_empty_env("SESSION_ENCRYPTION_KEY_FILE")?
        .unwrap_or_else(|| DEFAULT_SESSION_KEY_FILE.to_string());
    let path = Path::new(&path);
    if path.exists() {
        read_key(path)
    } else {
        let key = create_key(path)?;
        tracing::info!(path = %path.display(), "generated persistent session encryption key");
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_key_roundtrips_through_file() {
        let path = std::env::temp_dir().join(format!(
            "brain-ui-session-key-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = fs::remove_file(&path);

        let generated = create_key(&path).unwrap();
        let loaded = read_key(&path).unwrap();
        assert_eq!(generated.master(), loaded.master());

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn decode_rejects_short_keys() {
        let encoded = base64::engine::general_purpose::STANDARD.encode([0_u8; 32]);
        assert!(decode(&encoded).is_err());
    }

    #[test]
    fn whitespace_only_environment_values_are_unset() {
        assert_eq!(non_empty(String::new()), None);
        assert_eq!(non_empty(" \n\t ".into()), None);
        assert_eq!(
            non_empty("data/session.key".into()).as_deref(),
            Some("data/session.key")
        );
    }
}
