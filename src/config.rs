use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, anyhow};
use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::cli::SweepArgs;

const DEFAULT_PATH: &str = ".";

#[derive(Debug)]
pub struct SweepConfig {
    pub paths: Vec<PathBuf>,
    pub recursive: bool,
    pub select_gone: bool,
    pub select_merged: bool,
    pub require_merge_base: bool,
    pub select_all: bool,
    pub merge_base_ref: Option<String>,
    pub older_than: Option<Duration>,
    pub include_branch: Option<GlobSet>,
    pub exclude_branch: Option<GlobSet>,
    pub include_path: Option<GlobSet>,
    pub exclude_path: Option<GlobSet>,
    pub force: bool,
    pub force_with_dirty: bool,
    pub delete_branch: bool,
    pub json: bool,
}

impl SweepConfig {
    pub fn from_args(args: SweepArgs) -> Result<Self> {
        let has_explicit_merged_selector = args.merged || args.merged_to.is_some();
        let has_reason_selector =
            args.gone || has_explicit_merged_selector || args.older_than.is_some() || args.all;

        Ok(Self {
            paths: if args.paths.is_empty() {
                vec![PathBuf::from(DEFAULT_PATH)]
            } else {
                args.paths
            },
            recursive: args.recursive,
            select_gone: args.gone || !has_reason_selector,
            select_merged: has_explicit_merged_selector || !has_reason_selector,
            require_merge_base: has_explicit_merged_selector,
            select_all: args.all,
            merge_base_ref: args.merged_to,
            older_than: args.older_than,
            include_branch: compile_globset(&args.branch_globs, "branch")?,
            exclude_branch: compile_globset(&args.exclude_branch_globs, "exclude-branch")?,
            include_path: compile_globset(&args.path_globs, "path")?,
            exclude_path: compile_globset(&args.exclude_path_globs, "exclude-path")?,
            force: args.force,
            force_with_dirty: args.force_with_dirty,
            delete_branch: args.delete_branch,
            json: args.json,
        })
    }
}

fn compile_globset(patterns: &[String], label: &str) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }

    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern)
            .map_err(|error| anyhow!("invalid {label} glob `{pattern}`: {error}"))?;
        builder.add(glob);
    }

    Ok(Some(builder.build()?))
}
