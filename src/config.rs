use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

/// Resolves the database path: explicit `--db`/`GURD_DB` first, then the platform data directory.
pub fn database_path(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_owned());
    }
    Ok(data_dir()?.join("gurd.db"))
}

/// Cache directory: `$XDG_CACHE_HOME/gurd`, falling back to platform cache directories:
/// - Windows: `%LOCALAPPDATA%\gurd\cache`
/// - macOS: `~/Library/Caches/gurd`
/// - Linux/Unix: `~/.cache/gurd`
pub fn cache_dir() -> Result<PathBuf> {
    if let Some(dir) = absolute_env("XDG_CACHE_HOME") {
        return Ok(dir.join("gurd"));
    }
    #[cfg(windows)]
    if let Some(dir) = absolute_env("LOCALAPPDATA") {
        return Ok(dir.join("gurd").join("cache"));
    }
    let home = home_dir()
        .ok_or_else(|| anyhow!("cannot locate the cache directory (HOME is not set)"))?;
    #[cfg(target_os = "macos")]
    {
        Ok(home.join("Library").join("Caches").join("gurd"))
    }
    #[cfg(windows)]
    {
        Ok(home.join("AppData").join("Local").join("gurd").join("cache"))
    }
    #[cfg(all(not(target_os = "macos"), not(windows)))]
    {
        Ok(home.join(".cache").join("gurd"))
    }
}

/// Data directory: `$XDG_DATA_HOME/gurd`, falling back to platform data directories:
/// - Windows: `%LOCALAPPDATA%\gurd` or `%APPDATA%\gurd`
/// - macOS: `~/Library/Application Support/gurd`
/// - Linux/Unix: `~/.local/share/gurd`
pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = absolute_env("XDG_DATA_HOME") {
        return Ok(dir.join("gurd"));
    }
    #[cfg(windows)]
    if let Some(dir) = absolute_env("LOCALAPPDATA").or_else(|| absolute_env("APPDATA")) {
        return Ok(dir.join("gurd"));
    }
    let home = home_dir()
        .ok_or_else(|| anyhow!("cannot locate the data directory (HOME is not set); use --db"))?;
    #[cfg(target_os = "macos")]
    {
        Ok(home.join("Library").join("Application Support").join("gurd"))
    }
    #[cfg(windows)]
    {
        Ok(home.join("AppData").join("Local").join("gurd"))
    }
    #[cfg(all(not(target_os = "macos"), not(windows)))]
    {
        Ok(home.join(".local").join("share").join("gurd"))
    }
}

fn home_dir() -> Option<PathBuf> {
    absolute_env("HOME").or_else(|| absolute_env("USERPROFILE"))
}

// Relative values must be ignored.
fn absolute_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}
