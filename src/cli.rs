use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};

use crate::duration::parse_duration_spec;

#[derive(Debug, Parser)]
#[command(name = "gwt-sweep", version, about = "Clean stale Git worktrees")]
pub struct Cli {
    #[command(subcommand)]
    pub command: CliCommand,
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Find and remove Git worktrees that match cleanup selectors.
    Sweep(SweepArgs),
}

#[derive(Clone, Debug, Args)]
pub struct SweepArgs {
    /// Git repository paths to inspect.
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Recursively discover Git repositories below each path.
    #[arg(short, long)]
    pub recursive: bool,

    /// Select worktrees whose local branch tracks a gone upstream.
    #[arg(long)]
    pub gone: bool,

    /// Select worktrees whose HEAD is merged into the merge base.
    #[arg(long)]
    pub merged: bool,

    /// Select worktrees merged into this ref.
    #[arg(long = "merged-to", value_name = "REF")]
    pub merged_to: Option<String>,

    /// Select worktrees whose latest filesystem mtime is older than the duration.
    #[arg(long = "older-than", value_name = "DURATION", value_parser = parse_duration_spec)]
    pub older_than: Option<Duration>,

    /// Keep only worktrees whose branch matches the glob.
    #[arg(long = "branch", value_name = "GLOB")]
    pub branch_globs: Vec<String>,

    /// Exclude worktrees whose branch matches the glob.
    #[arg(long = "exclude-branch", value_name = "GLOB")]
    pub exclude_branch_globs: Vec<String>,

    /// Keep only worktrees whose path or basename matches the glob.
    #[arg(long = "include-path", alias = "path", value_name = "GLOB")]
    pub path_globs: Vec<String>,

    /// Exclude worktrees whose path or basename matches the glob.
    #[arg(long = "exclude-path", value_name = "GLOB")]
    pub exclude_path_globs: Vec<String>,

    /// Select all worktrees before safety checks, then apply filters.
    #[arg(long)]
    pub all: bool,

    /// Actually remove matched worktrees.
    #[arg(long)]
    pub force: bool,

    /// Allow dirty worktrees to be removed when --force is also set.
    #[arg(long = "force-with-dirty")]
    pub force_with_dirty: bool,

    /// Delete the local branch after a successful worktree removal.
    #[arg(long = "delete-branch")]
    pub delete_branch: bool,

    /// Print a machine-readable JSON report.
    #[arg(long)]
    pub json: bool,
}

#[cfg(test)]
pub fn default_args() -> SweepArgs {
    SweepArgs {
        paths: Vec::new(),
        recursive: false,
        gone: false,
        merged: false,
        merged_to: None,
        older_than: None,
        branch_globs: Vec::new(),
        exclude_branch_globs: Vec::new(),
        path_globs: Vec::new(),
        exclude_path_globs: Vec::new(),
        all: false,
        force: false,
        force_with_dirty: false,
        delete_branch: false,
        json: false,
    }
}
