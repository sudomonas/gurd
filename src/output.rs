use std::io::{self, IsTerminal, Write};

use anyhow::Result;
use serde::Serialize;

use crate::cli::ColorChoice;

/// Version of the JSON output format, included in every JSON document.
pub const JSON_VERSION: u32 = 1;

/// Writes to stdout. A closed pipe (`gurd x | head`) is not an error.
pub fn emit(text: &str) -> Result<()> {
    let mut out = io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Err(e) if e.kind() != io::ErrorKind::BrokenPipe => Err(e.into()),
        _ => Ok(()),
    }
}

pub fn emit_json<T: Serialize>(value: &T) -> Result<()> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    emit(&text)
}

/// Terminal styling. Color is used only for emphasis; output reads the same without it.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    color: bool,
}

impl Style {
    pub fn new(choice: ColorChoice, no_color: bool) -> Self {
        let color = !no_color
            && match choice {
                ColorChoice::Always => true,
                ColorChoice::Never => false,
                ColorChoice::Auto => {
                    std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
                        && std::env::var_os("TERM").is_none_or(|t| t != "dumb")
                        && io::stdout().is_terminal()
                }
            };
        Self { color }
    }

    pub fn plain() -> Self {
        Self { color: false }
    }

    pub fn bold(&self, s: &str) -> String {
        self.wrap("1", s)
    }

    pub fn dim(&self, s: &str) -> String {
        self.wrap("2", s)
    }

    fn wrap(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_owned()
        }
    }
}

/// Shows long output through `$PAGER` (default `less -FRX`) when stdout is a terminal.
/// Falls back to plain output if the pager cannot be started.
pub fn page(text: &str, enabled: bool) -> Result<()> {
    if !enabled || !io::stdout().is_terminal() {
        return emit(text);
    }
    let pager = std::env::var("PAGER").ok().filter(|p| !p.trim().is_empty());
    let mut command = match &pager {
        Some(p) => {
            let mut c = std::process::Command::new("sh");
            c.arg("-c").arg(p);
            c
        }
        None => {
            let mut c = std::process::Command::new("less");
            c.arg("-FRX");
            c
        }
    };
    let Ok(mut child) = command.stdin(std::process::Stdio::piped()).spawn() else {
        return emit(text);
    };
    if let Some(mut stdin) = child.stdin.take() {
        // The user quitting the pager early closes the pipe; that is not an error.
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait()?;
    Ok(())
}
