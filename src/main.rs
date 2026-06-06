mod cli;
mod config;
mod duration;
mod fs_scan;
mod git;
mod report;
mod sweep;

use std::io::{self, IsTerminal};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};

use crate::cli::Cli;
use crate::config::SweepConfig;
use crate::report::print_text_report;
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
    let progress = start_progress(&config);
    let report = match build_report(&config, &current_dir) {
        Ok(report) => {
            clear_progress(&progress);
            report
        }
        Err(error) => {
            clear_progress(&progress);
            return Err(error);
        }
    };

    if config.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text_report(&report, config.verbose)?;
    }

    if report.summary.errors > 0 {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn start_progress(config: &SweepConfig) -> Option<ProgressBar> {
    if config.json || !io::stderr().is_terminal() {
        return None;
    }

    let progress = ProgressBar::new_spinner();
    progress.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg}")
            .expect("progress style template should be valid"),
    );
    progress.set_message("Sweeping worktrees...");
    progress.enable_steady_tick(Duration::from_millis(80));
    Some(progress)
}

fn clear_progress(progress: &Option<ProgressBar>) {
    if let Some(progress) = progress {
        progress.finish_and_clear();
    }
}
