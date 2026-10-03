use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

/// Resolves the database path: explicit `--db`/`GURD_DB` first, then the XDG data directory.
pub fn database_path(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_owned());
    }
    Ok(data_dir()?.join("gurd.db"))
}

/// `$XDG_CACHE_HOME/gurd`, falling back to `~/.cache/gurd`. Holds downloads in progress.
pub fn cache_dir() -> Result<PathBuf> {
    if let Some(dir) = absolute_env("XDG_CACHE_HOME") {
        return Ok(dir.join("gurd"));
    }
    let home = absolute_env("HOME")
        .ok_or_else(|| anyhow!("cannot locate the cache directory (HOME is not set)"))?;
    Ok(home.join(".cache").join("gurd"))
}

/// `$XDG_DATA_HOME/gurd`, falling back to `~/.local/share/gurd`.
pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = absolute_env("XDG_DATA_HOME") {
        return Ok(dir.join("gurd"));
    }
    let home = absolute_env("HOME")
        .ok_or_else(|| anyhow!("cannot locate the data directory (HOME is not set); use --db"))?;
    Ok(home.join(".local").join("share").join("gurd"))
}

// The XDG spec says relative values must be ignored.
fn absolute_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}
