mod cli;
mod config;
mod duration;
mod fs_scan;
mod git;
mod report;
mod sweep;

use std::io::{self, IsTerminal};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};

use crate::cli::Cli;
use crate::config::SweepConfig;
use crate::report::print_text_report;
use crate::sweep::{SweepProgress, build_report_with_progress};

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
    let progress = TextProgress::new(&config);
    let report = match build_report_with_progress(&config, &current_dir, &progress) {
        Ok(report) => {
            progress.clear();
            report
        }
        Err(error) => {
            progress.clear();
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

struct TextProgress {
    bar: Option<ProgressBar>,
}

impl TextProgress {
    fn new(config: &SweepConfig) -> Self {
        if config.json || !io::stderr().is_terminal() {
            return Self { bar: None };
        }

        let bar = ProgressBar::new(1);
        bar.set_style(
            ProgressStyle::with_template("{wide_bar:.cyan/blue} {pos:>3}/{len:<3} {msg}")
                .expect("progress style template should be valid")
                .progress_chars("=>-"),
        );
        Self { bar: Some(bar) }
    }

    fn clear(&self) {
        if let Some(bar) = &self.bar {
            bar.finish_and_clear();
        }
    }
}

impl SweepProgress for TextProgress {
    fn begin_discovery(&self) {
        if let Some(bar) = &self.bar {
            bar.set_length(1);
            bar.set_position(0);
            bar.set_message("Discovering repositories");
        }
    }

    fn repositories_discovered(&self, total: usize) {
        if let Some(bar) = &self.bar {
            bar.set_length(total.max(1) as u64);
            bar.set_position(0);
            if total == 0 {
                bar.set_message("No repositories found");
            } else {
                bar.set_message("Inspecting repositories");
            }
        }
    }

    fn begin_repository(&self, path: &std::path::Path) {
        if let Some(bar) = &self.bar {
            bar.set_message(format!("Inspecting {}", path.display()));
        }
    }

    fn finish_repository(&self) {
        if let Some(bar) = &self.bar {
            bar.inc(1);
        }
    }
}
