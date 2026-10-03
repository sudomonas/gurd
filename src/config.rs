use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

/// Resolves the database path: explicit `--db`/`DRUG_DB` first, then the XDG data directory.
pub fn database_path(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_owned());
    }
    Ok(data_dir()?.join("drug.db"))
}

/// `$XDG_CACHE_HOME/drug`, falling back to `~/.cache/drug`. Holds downloads in progress.
pub fn cache_dir() -> Result<PathBuf> {
    if let Some(dir) = absolute_env("XDG_CACHE_HOME") {
        return Ok(dir.join("drug"));
    }
    let home = absolute_env("HOME")
        .ok_or_else(|| anyhow!("cannot locate the cache directory (HOME is not set)"))?;
    Ok(home.join(".cache").join("drug"))
}

/// `$XDG_DATA_HOME/drug`, falling back to `~/.local/share/drug`.
pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = absolute_env("XDG_DATA_HOME") {
        return Ok(dir.join("drug"));
    }
    let home = absolute_env("HOME")
        .ok_or_else(|| anyhow!("cannot locate the data directory (HOME is not set); use --db"))?;
    Ok(home.join(".local").join("share").join("drug"))
}

// The XDG spec says relative values must be ignored.
fn absolute_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}
