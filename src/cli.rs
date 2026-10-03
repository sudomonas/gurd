use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

const AFTER_HELP: &str = "\
Examples:
  gurd metformin
  gurd \"amoxicillin clavulanate\"
  gurd metformin --json
  gurd rxcui 6809
  gurd database

Exit status: 0 success, 1 not found or error, 2 invalid usage.
Lookups never use the network; only `gurd update` does.";

#[derive(Debug, Parser)]
#[command(
    name = "gurd",
    version,
    about = "Local-first drug lookup",
    after_help = AFTER_HELP,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(flatten)]
    pub search: SearchArgs,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Args)]
pub struct GlobalArgs {
    /// Database file [default: $XDG_DATA_HOME/gurd/gurd.db]
    #[arg(long, global = true, env = "GURD_DB", value_name = "PATH")]
    pub db: Option<PathBuf>,

    /// Print JSON instead of text
    #[arg(long, global = true)]
    pub json: bool,

    /// When to use color
    #[arg(long, global = true, value_name = "WHEN", default_value = "auto")]
    pub color: ColorChoice,

    /// Never use color (same as --color never)
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Never page output
    #[arg(long, global = true)]
    pub no_pager: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Drug name to search for; several words are joined with spaces
    #[arg(value_name = "QUERY")]
    pub query: Vec<String>,

    /// Show the detailed view of the best match
    #[arg(short, long)]
    pub details: bool,

    /// Maximum number of results
    #[arg(long, value_name = "N", default_value_t = 20)]
    pub limit: usize,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Search by name (use when the name collides with a subcommand)
    Search(SearchArgs),

    /// Show the detailed view of the best match
    Show {
        #[arg(required = true, value_name = "QUERY")]
        query: Vec<String>,
    },

    /// Look up a concept by RxCUI
    Rxcui {
        #[arg(value_name = "RXCUI")]
        rxcui: String,
    },

    /// Look up concepts by identifier, e.g. rxcui:6809 or unii:9100L32L2N
    Id {
        #[arg(value_name = "SYSTEM:VALUE")]
        identifier: String,
    },

    /// Show drug classes for a drug
    Class {
        #[arg(required = true, value_name = "QUERY")]
        query: Vec<String>,
    },

    /// Show the database location and installed source versions
    #[command(visible_alias = "info")]
    Database,

    /// List the data sources this build supports, their licenses and what is installed
    Sources,

    /// Download, import and install source datasets (uses the network)
    Update(UpdateArgs),

    /// Remove an installed source and rebuild the database without it
    Remove {
        #[arg(value_name = "SOURCE")]
        source: String,
    },
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Import from a local release file or directory instead of downloading
    #[arg(long, value_name = "FILE", conflicts_with = "url")]
    pub from: Option<PathBuf>,

    /// Download this release file instead of the source's default
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    /// Source to install or update (see `gurd sources`)
    #[arg(long, value_name = "SOURCE", default_value = "rxnorm")]
    pub source: String,

    /// Verify the release file against this MD5 checksum, as published by the provider
    #[arg(long, value_name = "HEX")]
    pub md5: Option<String>,

    /// Reinstall even if this release is already installed
    #[arg(long)]
    pub force: bool,

    /// Keep the downloaded file in the cache directory
    #[arg(long)]
    pub keep_download: bool,
}

impl SearchArgs {
    pub fn query_string(&self) -> String {
        self.query.join(" ")
    }
}
