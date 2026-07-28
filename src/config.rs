//! Session persistence: `~/.config/protein/sessions.json`.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::models::{CaffeineSession, SessionStatus};

const APP_DIR: &str = "protein";
const SESSIONS_FILE: &str = "sessions.json";

/// `$XDG_CONFIG_HOME/protein/sessions.json`, defaulting to `~/.config`.
///
/// Deliberately not `dirs::config_dir()`: on macOS that resolves to
/// `~/Library/Application Support`, and this tool's config belongs next to the
/// rest of a developer's dotfiles.
pub fn sessions_path() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        Some(value) => PathBuf::from(value),
        None => dirs::home_dir()
            .context("could not resolve a home directory")?
            .join(".config"),
    };
    Ok(base.join(APP_DIR).join(SESSIONS_FILE))
}

/// Read the saved sessions. Anything recorded as `Running` is downgraded to
/// `Stopped`: the PID belonged to a previous run of this process and is gone.
///
/// A missing or corrupt file yields an empty list rather than an error — a bad
/// save must never stop the app from starting.
pub fn load() -> Vec<CaffeineSession> {
    let Ok(path) = sessions_path() else {
        return Vec::new();
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(mut sessions) = serde_json::from_str::<Vec<CaffeineSession>>(&raw) else {
        return Vec::new();
    };

    for session in &mut sessions {
        if session.status == SessionStatus::Running {
            session.status = SessionStatus::Stopped;
        }
        session.pid = None;
        session.expires_at = None;
    }
    sessions
}

/// Write via temp file + rename so an interrupted save cannot truncate the
/// existing list.
pub fn save(sessions: &[CaffeineSession]) -> Result<()> {
    let path = sessions_path()?;
    let parent = path
        .parent()
        .context("sessions path has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;

    let json = serde_json::to_string_pretty(sessions)?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, json).with_context(|| format!("writing {}", temp.display()))?;
    fs::rename(&temp, &path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}
