mod cli;
mod config;
mod duration;
mod fs_scan;
mod git;
mod report;
mod sweep;

use std::cell::RefCell;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};

use crate::cli::Cli;
use crate::config::SweepConfig;
use crate::git::DiscoveryProgress;
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
    enabled: bool,
    bar: RefCell<Option<ProgressBar>>,
}

impl TextProgress {
    fn new(config: &SweepConfig) -> Self {
        if config.json || !io::stderr().is_terminal() {
            return Self {
                enabled: false,
                bar: RefCell::new(None),
            };
        }

        Self {
            enabled: true,
            bar: RefCell::new(None),
        }
    }

    fn clear(&self) {
        if let Some(bar) = self.bar.borrow_mut().take() {
            bar.finish_and_clear();
        }
    }

    fn start_bar(&self, length: u64, message: impl Into<String>) {
        if !self.enabled {
            return;
        }

        self.clear();
        let bar = ProgressBar::new(length);
        bar.set_style(
            ProgressStyle::with_template("{wide_bar:.cyan/blue} {pos:>5}/{len:<5} {msg}")
                .expect("progress style template should be valid")
                .progress_chars("=>-"),
        );
        bar.set_message(message.into());
        *self.bar.borrow_mut() = Some(bar);
    }

    fn start_spinner(&self, message: impl Into<String>) {
        if !self.enabled {
            return;
        }

        self.clear();
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::with_template("{spinner:.green} {msg} ({pos} scanned)")
                .expect("progress style template should be valid"),
        );
        bar.set_message(message.into());
        bar.enable_steady_tick(Duration::from_millis(80));
        *self.bar.borrow_mut() = Some(bar);
    }
}

impl DiscoveryProgress for TextProgress {
    fn directory_discovered(&self, _path: &Path) {}

    fn directory_scanned(&self, _path: &Path) {
        if let Some(bar) = self.bar.borrow().as_ref() {
            bar.inc(1);
        }
    }
}

impl SweepProgress for TextProgress {
    fn begin_discovery(&self) {
        self.start_spinner("Discovering repositories");
    }

    fn repositories_discovered(&self, total: usize) {
        if total == 0 {
            self.clear();
        } else {
            self.start_bar(total as u64, "Inspecting repositories");
        }
    }

    fn begin_repository(&self, path: &Path) {
        if let Some(bar) = self.bar.borrow().as_ref() {
            bar.set_message(format!("Inspecting {}", path.display()));
        }
    }

    fn finish_repository(&self) {
        if let Some(bar) = self.bar.borrow().as_ref() {
            bar.inc(1);
        }
    }
}
