use std::process::ExitCode;

use clap::{CommandFactory, Parser};

fn main() -> ExitCode {
    // Usage errors exit with status 2 inside `parse`.
    let cli = gurd::cli::Cli::parse();

    // Options alone (or GURD_DB in the environment) are not a command.
    if cli.command.is_none() && cli.search.query.is_empty() {
        let help = gurd::cli::Cli::command().render_help();
        eprint!("{help}");
        return ExitCode::from(2);
    }

    match gurd::app::run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("gurd: {err:#}");
            ExitCode::from(1)
        }
    }
}
