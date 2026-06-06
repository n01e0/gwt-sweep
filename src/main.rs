mod cli;
mod config;
mod duration;
mod fs_scan;
mod git;
mod report;
mod sweep;

use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;

use crate::cli::Cli;
use crate::config::SweepConfig;
use crate::report::print_human_report;
use crate::sweep::build_report;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run_cli(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(1)
        }
    }
}

fn run_cli(cli: Cli) -> Result<ExitCode> {
    let config = SweepConfig::from_args(cli)?;
    let current_dir = std::env::current_dir().context("failed to read current directory")?;
    let report = build_report(&config, &current_dir)?;

    if config.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_report(&report, config.verbose)?;
    }

    if report.summary.errors > 0 {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}
