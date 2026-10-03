//! `gurd`: a local-first terminal drug lookup.
//!
//! Layering: `cli` (argument parsing) → `app` (commands) → `database` (SQLite repository).
//! Lookups never touch the network and open the database read-only.

pub mod app;
pub mod cli;
pub mod config;
pub mod database;
pub mod details;
pub mod import;
pub mod models;
pub mod normalize;
pub mod output;
pub mod render;
pub mod search;
pub mod sources;
pub mod update;

#[cfg(feature = "net")]
pub mod net;
