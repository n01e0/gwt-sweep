use std::path::Path;

use anyhow::Result;
use git2::Repository;

use crate::config::SweepConfig;
use crate::duration::format_duration;
use crate::fs_scan::is_older_than;
use crate::git::{
    DiscoveryProgress, MergeBase, RepoInfo, WorktreeInfo, branch_tracks_gone_upstream,
    check_branch_delete_safety, current_worktree, delete_branch_safely,
    discover_repositories_with_progress, is_ancestor, is_worktree_dirty, list_worktrees, open_repo,
    remove_worktree, same_path,
};
use crate::report::{
    ACTION_ERROR, ACTION_REMOVED, ACTION_SKIP, ACTION_WOULD_REMOVE, KIND_REPOSITORY_ERROR,
    KIND_WORKTREE, SweepItem, SweepReport, SweepSummary, update_summary,
};

#[derive(Debug)]
struct RepoContext {
    merge_base: Option<MergeBase>,
    branch_delete_base: Option<MergeBase>,
}

#[derive(Debug)]
struct Evaluation {
    item: SweepItem,
    can_remove: bool,
    branch_to_delete: Option<String>,
    worktree_name: String,
}

#[derive(Debug)]
struct ReasonSet {
    reasons: Vec<String>,
    errors: Vec<String>,
}

pub trait SweepProgress: DiscoveryProgress {
    fn begin_discovery(&self) {}
    fn repositories_discovered(&self, _total: usize) {}
    fn begin_repository(&self, _path: &Path) {}
    fn finish_repository(&self) {}
}

#[cfg(test)]
struct NoProgress;

#[cfg(test)]
impl SweepProgress for NoProgress {}

#[cfg(test)]
impl DiscoveryProgress for NoProgress {}

#[cfg(test)]
pub fn build_report(config: &SweepConfig, cwd: &Path) -> Result<SweepReport> {
    build_report_with_progress(config, cwd, &NoProgress)
}

pub fn build_report_with_progress(
    config: &SweepConfig,
    cwd: &Path,
    progress: &impl SweepProgress,
) -> Result<SweepReport> {
    progress.begin_discovery();
    let discovery = discover_repositories_with_progress(
        &config.paths,
        config.recursive,
        config.include_hidden,
        progress,
    );
    progress.repositories_discovered(discovery.repos.len());
    let current = current_worktree(cwd)?;
    let mut report = SweepReport {
        items: Vec::new(),
        summary: SweepSummary {
            dry_run: !config.force,
            repositories: discovery.repos.len(),
            ..SweepSummary::default()
        },
    };

    for error in discovery.errors {
        let item = error_item(&error.path, &error.path, vec!["repo:error"], error.message);
        update_summary(&mut report.summary, &item);
        report.items.push(item);
    }

    for repo_info in discovery.repos {
        progress.begin_repository(&repo_info.path);
        let result = process_repository(&repo_info, config, current.as_deref(), &mut report);
        progress.finish_repository();
        result?;
    }

    Ok(report)
}

fn process_repository(
    repo_info: &RepoInfo,
    config: &SweepConfig,
    current: Option<&Path>,
    report: &mut SweepReport,
) -> Result<()> {
    let repo = match open_repo(&repo_info.path) {
        Ok(repo) => repo,
        Err(error) => {
            let item = error_item(
                &repo_info.path,
                &repo_info.path,
                vec!["repo:error"],
                format!("{error:#}"),
            );
            update_summary(&mut report.summary, &item);
            report.items.push(item);
            return Ok(());
        }
    };
    let merge_base = if config.select_merged {
        match crate::git::resolve_merge_base(&repo, config.merge_base_ref.as_deref()) {
            Ok(Some(base)) => Some(base),
            Ok(None) if config.require_merge_base => {
                let item = error_item(
                    &repo_info.path,
                    &repo_info.path,
                    vec!["merged:error"],
                    "failed to resolve merge base; pass --merged-to <ref>".to_owned(),
                );
                update_summary(&mut report.summary, &item);
                report.items.push(item);
                return Ok(());
            }
            Ok(None) => None,
            Err(error) if config.require_merge_base => {
                let item = error_item(
                    &repo_info.path,
                    &repo_info.path,
                    vec!["repo:error"],
                    format!("{error:#}"),
                );
                update_summary(&mut report.summary, &item);
                report.items.push(item);
                return Ok(());
            }
            Err(_) => None,
        }
    } else {
        None
    };
    let branch_delete_base = if config.delete_branch {
        match merge_base.clone() {
            Some(base) => Some(base),
            None => match crate::git::resolve_merge_base(&repo, config.merge_base_ref.as_deref()) {
                Ok(base) => base,
                Err(error) if config.require_merge_base => {
                    let item = error_item(
                        &repo_info.path,
                        &repo_info.path,
                        vec!["repo:error"],
                        format!("{error:#}"),
                    );
                    update_summary(&mut report.summary, &item);
                    report.items.push(item);
                    return Ok(());
                }
                Err(_) => None,
            },
        }
    } else {
        None
    };
    let context = RepoContext {
        merge_base,
        branch_delete_base,
    };
    let worktrees = match list_worktrees(&repo) {
        Ok(worktrees) => worktrees,
        Err(error) => {
            let item = error_item(
                &repo_info.path,
                &repo_info.path,
                vec!["repo:error"],
                format!("{error:#}"),
            );
            update_summary(&mut report.summary, &item);
            report.items.push(item);
            return Ok(());
        }
    };

    for worktree in worktrees {
        let Some(mut evaluation) =
            evaluate_worktree(&repo, &repo_info.path, &worktree, &context, config, current)?
        else {
            continue;
        };

        if config.force && evaluation.can_remove {
            remove_and_maybe_delete_branch(&repo, &mut evaluation, &context);
        }

        update_summary(&mut report.summary, &evaluation.item);
        report.items.push(evaluation.item);
    }

    Ok(())
}

fn error_item(repo: &Path, path: &Path, reasons: Vec<&str>, error: String) -> SweepItem {
    SweepItem {
        kind: KIND_REPOSITORY_ERROR.to_owned(),
        repo: repo.display().to_string(),
        path: path.display().to_string(),
        branch: None,
        head: None,
        reasons: reasons.into_iter().map(ToOwned::to_owned).collect(),
        dirty: false,
        locked: false,
        action: ACTION_ERROR.to_owned(),
        skip_reason: None,
        error: Some(error),
        branch_deleted: false,
        branch_action: None,
        branch_error: None,
    }
}

fn evaluate_worktree(
    repo: &Repository,
    repo_path: &Path,
    worktree: &WorktreeInfo,
    context: &RepoContext,
    config: &SweepConfig,
    current_worktree: Option<&Path>,
) -> Result<Option<Evaluation>> {
    let is_current = current_worktree.is_some_and(|current| same_path(current, &worktree.path));
    if is_current && !config.select_all {
        return Ok(None);
    }

    let mut reason_set = collect_reasons(repo, worktree, context, config)?;
    if reason_set.reasons.is_empty() {
        return Ok(None);
    }

    if !matches_filters(worktree.branch.as_deref(), &worktree.path, config) {
        return Ok(None);
    }

    let mut item = SweepItem {
        kind: KIND_WORKTREE.to_owned(),
        repo: repo_path.display().to_string(),
        path: worktree.path.display().to_string(),
        branch: worktree.branch.clone(),
        head: worktree.head.map(|head| head.to_string()),
        reasons: std::mem::take(&mut reason_set.reasons),
        dirty: false,
        locked: worktree.locked,
        action: ACTION_SKIP.to_owned(),
        skip_reason: None,
        error: None,
        branch_deleted: false,
        branch_action: None,
        branch_error: None,
    };

    if !reason_set.errors.is_empty() {
        item.action = ACTION_ERROR.to_owned();
        item.error = Some(reason_set.errors.join("; "));
        return Ok(Some(Evaluation {
            item,
            can_remove: false,
            branch_to_delete: None,
            worktree_name: worktree.name.clone(),
        }));
    }

    if is_current {
        item.skip_reason = Some("current worktree is protected".to_owned());
        return Ok(Some(Evaluation {
            item,
            can_remove: false,
            branch_to_delete: None,
            worktree_name: worktree.name.clone(),
        }));
    }

    if worktree.locked {
        item.skip_reason = Some("locked worktree is protected".to_owned());
        return Ok(Some(Evaluation {
            item,
            can_remove: false,
            branch_to_delete: None,
            worktree_name: worktree.name.clone(),
        }));
    }

    if worktree.prunable {
        item.skip_reason = Some("prunable worktree metadata is not removed by sweep".to_owned());
        return Ok(Some(Evaluation {
            item,
            can_remove: false,
            branch_to_delete: None,
            worktree_name: worktree.name.clone(),
        }));
    }

    if let Some(error) = &worktree.inspection_error {
        item.action = ACTION_ERROR.to_owned();
        item.error = Some(format!("failed to inspect worktree metadata: {error}"));
        return Ok(Some(Evaluation {
            item,
            can_remove: false,
            branch_to_delete: None,
            worktree_name: worktree.name.clone(),
        }));
    }

    let dirty = match is_worktree_dirty(&worktree.path) {
        Ok(dirty) => dirty,
        Err(error) => {
            item.action = ACTION_ERROR.to_owned();
            item.error = Some(format!("failed to inspect dirty state: {error:#}"));
            return Ok(Some(Evaluation {
                item,
                can_remove: false,
                branch_to_delete: None,
                worktree_name: worktree.name.clone(),
            }));
        }
    };
    item.dirty = dirty;

    if dirty && !config.force_with_dirty {
        item.skip_reason = Some("worktree has uncommitted or untracked changes".to_owned());
        return Ok(Some(Evaluation {
            item,
            can_remove: false,
            branch_to_delete: None,
            worktree_name: worktree.name.clone(),
        }));
    }

    if !config.force {
        item.action = ACTION_WOULD_REMOVE.to_owned();
    }

    let branch_to_delete = worktree
        .branch
        .as_ref()
        .filter(|_| config.delete_branch)
        .cloned();
    if let Some(branch) = &branch_to_delete {
        match &context.branch_delete_base {
            Some(base) => {
                if let Err(error) = check_branch_delete_safety(repo, branch, base) {
                    item.branch_action = Some(crate::report::BRANCH_ACTION_ERROR.to_owned());
                    item.branch_error = Some(format!("{error:#}"));
                } else if !config.force {
                    item.branch_action = Some(crate::report::BRANCH_ACTION_WOULD_DELETE.to_owned());
                }
            }
            None => {
                item.branch_action = Some(crate::report::BRANCH_ACTION_ERROR.to_owned());
                item.branch_error = Some("failed to resolve branch deletion base".to_owned());
            }
        }
    }

    Ok(Some(Evaluation {
        item,
        can_remove: true,
        branch_to_delete,
        worktree_name: worktree.name.clone(),
    }))
}

fn collect_reasons(
    repo: &Repository,
    worktree: &WorktreeInfo,
    context: &RepoContext,
    config: &SweepConfig,
) -> Result<ReasonSet> {
    let mut reasons = Vec::new();
    let mut errors = Vec::new();

    if config.select_all {
        reasons.push("all".to_owned());
    }

    if config.select_gone
        && let Some(branch) = &worktree.branch
    {
        match branch_tracks_gone_upstream(repo, branch) {
            Ok(true) => reasons.push("gone".to_owned()),
            Ok(false) => {}
            Err(error) => {
                reasons.push("gone:error".to_owned());
                errors.push(format!(
                    "failed to inspect gone upstream for branch `{branch}`: {error:#}"
                ));
            }
        }
    }

    if config.select_merged {
        match (&context.merge_base, worktree.head) {
            (Some(merge_base), Some(head)) => match is_ancestor(repo, head, merge_base.oid) {
                Ok(true) => reasons.push(format!("merged:{}", merge_base.display)),
                Ok(false) => {}
                Err(error) => {
                    reasons.push("merged:error".to_owned());
                    errors.push(format!(
                        "failed to inspect merge state against {}: {error:#}",
                        merge_base.display
                    ));
                }
            },
            (Some(_), None) if config.require_merge_base => {
                reasons.push("merged:error".to_owned());
                errors
                    .push("failed to inspect merge state: worktree HEAD is unavailable".to_owned());
            }
            (None, _) if config.require_merge_base => {
                reasons.push("merged:error".to_owned());
                errors.push("failed to resolve merge base; pass --merged-to <ref>".to_owned());
            }
            _ => {}
        }
    }

    if let Some(age) = config.older_than
        && worktree.path.exists()
    {
        match is_older_than(&worktree.path, age) {
            Ok(true) => reasons.push(format!("older_than:{}", format_duration(age))),
            Ok(false) => {}
            Err(error) => {
                reasons.push("older_than:error".to_owned());
                errors.push(format!(
                    "failed to scan mtimes under {}: {error}",
                    worktree.path.display()
                ));
            }
        }
    }

    Ok(ReasonSet { reasons, errors })
}

fn matches_filters(branch: Option<&str>, path: &Path, config: &SweepConfig) -> bool {
    if let Some(include) = &config.include_branch {
        let Some(branch) = branch else {
            return false;
        };
        if !include.is_match(branch) {
            return false;
        }
    }

    if let (Some(exclude), Some(branch)) = (&config.exclude_branch, branch)
        && exclude.is_match(branch)
    {
        return false;
    }

    let path_text = path.to_string_lossy();
    if let Some(include) = &config.include_path
        && !path_matches(include, path, path_text.as_ref())
    {
        return false;
    }

    if let Some(exclude) = &config.exclude_path
        && path_matches(exclude, path, path_text.as_ref())
    {
        return false;
    }

    true
}

fn path_matches(patterns: &globset::GlobSet, path: &Path, path_text: &str) -> bool {
    patterns.is_match(path_text)
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| patterns.is_match(name))
}

fn remove_and_maybe_delete_branch(
    repo: &Repository,
    evaluation: &mut Evaluation,
    context: &RepoContext,
) {
    match remove_worktree(repo, &evaluation.worktree_name) {
        Ok(()) => {
            evaluation.item.action = ACTION_REMOVED.to_owned();
        }
        Err(error) => {
            evaluation.item.action = ACTION_ERROR.to_owned();
            evaluation.item.error = Some(format!("{error:#}"));
            return;
        }
    }

    if let Some(branch) = &evaluation.branch_to_delete
        && let Some(base) = &context.branch_delete_base
        && let Err(error) = delete_branch_safely(repo, branch, base)
    {
        evaluation.item.branch_action = Some(crate::report::BRANCH_ACTION_ERROR.to_owned());
        evaluation.item.branch_error = Some(format!("{error:#}"));
    } else if evaluation.branch_to_delete.is_some() && context.branch_delete_base.is_none() {
        evaluation.item.branch_action = Some(crate::report::BRANCH_ACTION_ERROR.to_owned());
        evaluation.item.branch_error = Some("failed to resolve branch deletion base".to_owned());
    } else if evaluation.branch_to_delete.is_some() {
        evaluation.item.branch_action = Some(crate::report::BRANCH_ACTION_DELETED.to_owned());
        evaluation.item.branch_deleted = true;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::time::Duration;

    use git2::{BranchType, Repository, Signature};
    use tempfile::TempDir;

    use super::*;
    use crate::cli::default_args;
    use crate::config::SweepConfig;

    #[test]
    fn dry_run_reports_dirty_worktree_as_skip() {
        let fixture = GitFixture::new();
        fixture.create_branch("dirty");
        let dirty_path = fixture.temp.path().join("dirty-wt");
        fixture.add_worktree("dirty-wt", &dirty_path, "dirty");
        fs::write(dirty_path.join("untracked.txt"), "dirty").unwrap();

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            all: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();
        let dirty_item = report
            .items
            .iter()
            .find(|item| item.path == dirty_path.display().to_string())
            .unwrap();

        assert_eq!(dirty_item.action, ACTION_SKIP);
        assert!(dirty_item.dirty);
        assert_eq!(
            dirty_item.skip_reason.as_deref(),
            Some("worktree has uncommitted or untracked changes")
        );
        assert!(dirty_path.exists());
    }

    #[test]
    fn force_removes_merged_worktree_and_deletes_branch() {
        let fixture = GitFixture::new();
        fixture.create_branch("merged");
        let worktree_path = fixture.temp.path().join("merged-wt");
        fixture.add_worktree("merged-wt", &worktree_path, "merged");

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            merged: true,
            force: true,
            delete_branch: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();

        assert_eq!(report.summary.removed, 1);
        assert_eq!(report.summary.branch_deleted, 1);
        assert!(!worktree_path.exists());
        assert!(
            fixture
                .repo
                .find_branch("merged", BranchType::Local)
                .is_err()
        );
    }

    #[test]
    fn force_refuses_to_delete_merge_target_branch() {
        let fixture = GitFixture::new_with_initial_branch("trunk");
        fixture.create_branch("main");
        let worktree_path = fixture.temp.path().join("main-wt");
        fixture.add_worktree("main-wt", &worktree_path, "main");

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            merged: true,
            force: true,
            delete_branch: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();
        let item = report
            .items
            .iter()
            .find(|item| item.path == worktree_path.display().to_string())
            .unwrap();

        assert_eq!(item.action, ACTION_REMOVED);
        assert_eq!(
            item.branch_action.as_deref(),
            Some(crate::report::BRANCH_ACTION_ERROR)
        );
        assert!(
            item.branch_error
                .as_deref()
                .unwrap()
                .contains("merge target")
        );
        assert_eq!(report.summary.removed, 1);
        assert_eq!(report.summary.branch_deleted, 0);
        assert_eq!(report.summary.errors, 1);
        assert!(!worktree_path.exists());
        assert!(fixture.repo.find_branch("main", BranchType::Local).is_ok());
    }

    #[test]
    fn default_selectors_include_gone_branches() {
        let fixture = GitFixture::new();
        fixture.create_branch("gone");
        fixture.set_missing_upstream("gone");
        let worktree_path = fixture.temp.path().join("gone-wt");
        fixture.add_worktree("gone-wt", &worktree_path, "gone");

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();
        let gone_item = report
            .items
            .iter()
            .find(|item| item.path == worktree_path.display().to_string())
            .unwrap();

        assert_eq!(gone_item.action, ACTION_WOULD_REMOVE);
        assert!(gone_item.reasons.iter().any(|reason| reason == "gone"));
    }

    #[test]
    fn default_selectors_ignore_broken_origin_head_for_gone_branch() {
        let fixture = GitFixture::new();
        fixture.set_broken_origin_head();
        fixture.create_branch("gone-with-broken-origin-head");
        fixture.set_missing_upstream("gone-with-broken-origin-head");
        let worktree_path = fixture.temp.path().join("gone-with-broken-origin-head-wt");
        fixture.add_worktree(
            "gone-with-broken-origin-head-wt",
            &worktree_path,
            "gone-with-broken-origin-head",
        );

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();
        let gone_item = report
            .items
            .iter()
            .find(|item| item.path == worktree_path.display().to_string())
            .unwrap();

        assert_eq!(report.summary.errors, 0);
        assert_eq!(gone_item.action, ACTION_WOULD_REMOVE);
        assert!(gone_item.reasons.iter().any(|reason| reason == "gone"));
    }

    #[test]
    fn default_selectors_do_not_require_merge_base_for_gone_branch() {
        let fixture = GitFixture::new_with_initial_branch("trunk");
        fixture.create_branch("gone-on-trunk");
        fixture.set_missing_upstream("gone-on-trunk");
        let worktree_path = fixture.temp.path().join("gone-on-trunk-wt");
        fixture.add_worktree("gone-on-trunk-wt", &worktree_path, "gone-on-trunk");

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();
        let gone_item = report
            .items
            .iter()
            .find(|item| item.path == worktree_path.display().to_string())
            .unwrap();

        assert_eq!(report.summary.errors, 0);
        assert_eq!(gone_item.action, ACTION_WOULD_REMOVE);
        assert!(gone_item.reasons.iter().any(|reason| reason == "gone"));
    }

    #[test]
    fn recursive_discovery_deduplicates_worktrees_from_the_same_repo() {
        let fixture = GitFixture::new();
        fixture.create_branch("merged");
        let worktree_path = fixture.temp.path().join("merged-wt");
        fixture.add_worktree("merged-wt", &worktree_path, "merged");

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.temp.path().to_path_buf()],
            recursive: true,
            merged: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();

        assert_eq!(report.summary.repositories, 1);
        assert_eq!(report.summary.would_remove, 1);
    }

    #[test]
    fn recursive_discovery_skips_hidden_directories_by_default() {
        let (temp, worktree_path) = hidden_repo_with_merged_worktree();

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![temp.path().to_path_buf()],
            recursive: true,
            merged: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, temp.path()).unwrap();

        assert_eq!(report.summary.repositories, 0);
        assert_eq!(report.items.len(), 0);
        assert!(worktree_path.exists());
    }

    #[test]
    fn recursive_discovery_includes_hidden_directories_with_hidden_flag() {
        let (temp, worktree_path) = hidden_repo_with_merged_worktree();

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![temp.path().to_path_buf()],
            recursive: true,
            hidden: true,
            merged: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, temp.path()).unwrap();

        assert_eq!(report.summary.repositories, 1);
        assert_eq!(report.summary.would_remove, 1);
        assert_eq!(report.items[0].path, worktree_path.display().to_string());
    }

    #[test]
    fn build_report_reports_repository_progress() {
        let fixture = GitFixture::new();
        fixture.create_branch("merged-progress");
        let worktree_path = fixture.temp.path().join("merged-progress-wt");
        fixture.add_worktree("merged-progress-wt", &worktree_path, "merged-progress");
        let progress = RecordingProgress::default();

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            recursive: true,
            merged: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report_with_progress(&config, &fixture.repo_path, &progress).unwrap();

        assert_eq!(report.summary.repositories, 1);
        assert_eq!(
            progress.events.borrow().as_slice(),
            &[
                "begin_discovery".to_owned(),
                format!("directory_discovered:{}", fixture.repo_path.display()),
                format!("directory_scanned:{}", fixture.repo_path.display()),
                "repositories_discovered:1".to_owned(),
                format!("begin_repository:{}", fixture.repo_path.display()),
                "finish_repository".to_owned(),
            ]
        );
    }

    #[test]
    fn prunable_worktree_metadata_is_reported_without_aborting() {
        let fixture = GitFixture::new();
        fixture.create_branch("stale");
        let worktree_path = fixture.temp.path().join("stale-wt");
        fixture.add_worktree("stale-wt", &worktree_path, "stale");
        fs::remove_dir_all(&worktree_path).unwrap();

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            all: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();
        let stale_item = report
            .items
            .iter()
            .find(|item| item.path == worktree_path.display().to_string())
            .unwrap();

        assert_eq!(stale_item.action, ACTION_SKIP);
        assert_eq!(
            stale_item.skip_reason.as_deref(),
            Some("prunable worktree metadata is not removed by sweep")
        );
    }

    #[cfg(unix)]
    #[test]
    fn older_than_scan_error_is_reported_as_item_error() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = GitFixture::new();
        fixture.create_branch("scan-error");
        let worktree_path = fixture.temp.path().join("scan-error-wt");
        fixture.add_worktree("scan-error-wt", &worktree_path, "scan-error");
        let blocked_path = worktree_path.join("blocked");
        fs::create_dir(&blocked_path).unwrap();
        fs::set_permissions(&blocked_path, fs::Permissions::from_mode(0o0)).unwrap();

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            older_than: Some(Duration::from_secs(0)),
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();
        fs::set_permissions(&blocked_path, fs::Permissions::from_mode(0o700)).unwrap();

        let item = report
            .items
            .iter()
            .find(|item| item.path == worktree_path.display().to_string())
            .unwrap();
        assert_eq!(item.action, ACTION_ERROR);
        assert!(
            item.reasons
                .iter()
                .any(|reason| reason == "older_than:error")
        );
        assert!(
            item.error
                .as_deref()
                .unwrap()
                .contains("failed to scan mtimes")
        );
        assert_eq!(report.summary.errors, 1);
    }

    #[test]
    fn branch_delete_failure_is_reported_separately_after_worktree_removal() {
        let fixture = GitFixture::new();
        fixture.create_branch("unmerged");
        let worktree_path = fixture.temp.path().join("unmerged-wt");
        fixture.add_worktree("unmerged-wt", &worktree_path, "unmerged");
        commit_in_worktree(&worktree_path, "unmerged.txt", "unmerged\n", "unmerged");

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            all: true,
            force: true,
            delete_branch: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();
        let item = report
            .items
            .iter()
            .find(|item| item.path == worktree_path.display().to_string())
            .unwrap();

        assert_eq!(item.action, ACTION_REMOVED);
        assert_eq!(
            item.branch_action.as_deref(),
            Some(crate::report::BRANCH_ACTION_ERROR)
        );
        assert!(item.branch_error.as_deref().unwrap().contains("not merged"));
        assert_eq!(report.summary.removed, 1);
        assert_eq!(report.summary.errors, 1);
        assert!(!worktree_path.exists());
        assert!(
            fixture
                .repo
                .find_branch("unmerged", BranchType::Local)
                .is_ok()
        );
    }

    #[test]
    fn force_with_dirty_removes_dirty_worktree() {
        let fixture = GitFixture::new();
        fixture.create_branch("dirty-remove");
        let worktree_path = fixture.temp.path().join("dirty-remove-wt");
        fixture.add_worktree("dirty-remove-wt", &worktree_path, "dirty-remove");
        fs::write(worktree_path.join("untracked.txt"), "dirty").unwrap();

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            all: true,
            force: true,
            force_with_dirty: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();

        assert_eq!(report.summary.removed, 1);
        assert!(!worktree_path.exists());
    }

    #[test]
    fn delete_branch_without_safe_base_does_not_delete_unmerged_branch() {
        let fixture = GitFixture::new_with_initial_branch("trunk");
        fixture.create_branch("feature");
        let worktree_path = fixture.temp.path().join("feature-wt");
        fixture.add_worktree("feature-wt", &worktree_path, "feature");
        commit_in_worktree(&worktree_path, "feature.txt", "feature\n", "feature");

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![worktree_path.clone()],
            all: true,
            force: true,
            delete_branch: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();
        let item = report
            .items
            .iter()
            .find(|item| item.path == worktree_path.display().to_string())
            .unwrap();

        assert_eq!(item.action, ACTION_REMOVED);
        assert_eq!(
            item.branch_action.as_deref(),
            Some(crate::report::BRANCH_ACTION_ERROR)
        );
        assert!(
            item.branch_error
                .as_deref()
                .unwrap()
                .contains("failed to resolve branch deletion base")
        );
        assert!(!worktree_path.exists());
        assert!(
            fixture
                .repo
                .find_branch("feature", BranchType::Local)
                .is_ok()
        );
        assert_eq!(report.summary.errors, 1);
    }

    #[test]
    fn default_branch_delete_base_ignores_main_tag() {
        let fixture = GitFixture::new_with_initial_branch("trunk");
        fixture.create_branch("feature-tagged-main");
        let worktree_path = fixture.temp.path().join("feature-tagged-main-wt");
        fixture.add_worktree(
            "feature-tagged-main-wt",
            &worktree_path,
            "feature-tagged-main",
        );
        commit_in_worktree(
            &worktree_path,
            "feature-tagged-main.txt",
            "feature\n",
            "feature",
        );
        fixture.tag_branch_tip("main", "feature-tagged-main");

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![worktree_path.clone()],
            all: true,
            force: true,
            delete_branch: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();
        let item = report
            .items
            .iter()
            .find(|item| item.path == worktree_path.display().to_string())
            .unwrap();

        assert_eq!(item.action, ACTION_REMOVED);
        assert_eq!(
            item.branch_action.as_deref(),
            Some(crate::report::BRANCH_ACTION_ERROR)
        );
        assert!(
            item.branch_error
                .as_deref()
                .unwrap()
                .contains("failed to resolve branch deletion base")
        );
        assert!(
            fixture
                .repo
                .find_branch("feature-tagged-main", BranchType::Local)
                .is_ok()
        );
    }

    #[test]
    fn default_merged_base_ignores_main_tag() {
        let fixture = GitFixture::new_with_initial_branch("trunk");
        fixture.create_branch("feature-tagged-main");
        let worktree_path = fixture.temp.path().join("feature-tagged-main-wt");
        fixture.add_worktree(
            "feature-tagged-main-wt",
            &worktree_path,
            "feature-tagged-main",
        );
        commit_in_worktree(
            &worktree_path,
            "feature-tagged-main.txt",
            "feature\n",
            "feature",
        );
        fixture.tag_branch_tip("main", "feature-tagged-main");

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            merged: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();

        assert_eq!(report.summary.errors, 1);
        assert_eq!(report.summary.matched, 0);
        assert_eq!(report.items[0].kind, KIND_REPOSITORY_ERROR);
        assert!(
            report.items[0]
                .error
                .as_deref()
                .unwrap()
                .contains("--merged-to")
        );
    }

    #[test]
    fn missing_input_path_is_reported_in_json_shape() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing");
        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![missing.clone()],
            all: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, temp.path()).unwrap();

        assert_eq!(report.summary.repositories, 0);
        assert_eq!(report.summary.matched, 0);
        assert_eq!(report.summary.errors, 1);
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].action, ACTION_ERROR);
        assert_eq!(report.items[0].kind, KIND_REPOSITORY_ERROR);
        assert!(
            report.items[0]
                .reasons
                .iter()
                .any(|reason| reason == "repo:error")
        );
        assert!(
            report.items[0]
                .error
                .as_deref()
                .unwrap()
                .contains("failed to resolve path")
        );
    }

    #[test]
    fn existing_non_repo_path_is_reported_in_json_shape() {
        let temp = tempfile::tempdir().unwrap();
        let non_repo = temp.path().join("non-repo");
        fs::create_dir(&non_repo).unwrap();
        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![non_repo.clone()],
            all: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, temp.path()).unwrap();

        assert_eq!(report.summary.repositories, 0);
        assert_eq!(report.summary.matched, 0);
        assert_eq!(report.summary.errors, 1);
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].action, ACTION_ERROR);
        assert_eq!(report.items[0].kind, KIND_REPOSITORY_ERROR);
        assert!(
            report.items[0]
                .reasons
                .iter()
                .any(|reason| reason == "repo:error")
        );
        assert!(
            report.items[0]
                .error
                .as_deref()
                .unwrap()
                .contains("not inside a Git repository")
        );
    }

    #[test]
    fn merged_to_alone_selects_merged_worktrees() {
        let fixture = GitFixture::new();
        fixture.create_branch("merged-to-main");
        let worktree_path = fixture.temp.path().join("merged-to-main-wt");
        fixture.add_worktree("merged-to-main-wt", &worktree_path, "merged-to-main");

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            merged_to: Some("main".to_owned()),
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();
        let item = report
            .items
            .iter()
            .find(|item| item.path == worktree_path.display().to_string())
            .unwrap();

        assert_eq!(item.action, ACTION_WOULD_REMOVE);
        assert!(item.reasons.iter().any(|reason| reason == "merged:main"));
    }

    #[test]
    fn merged_without_resolvable_base_reports_error() {
        let fixture = GitFixture::new_with_initial_branch("trunk");
        fixture.create_branch("feature");
        let worktree_path = fixture.temp.path().join("feature-wt");
        fixture.add_worktree("feature-wt", &worktree_path, "feature");

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            merged: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();

        assert_eq!(report.summary.errors, 1);
        assert_eq!(report.summary.matched, 0);
        assert_eq!(report.items[0].kind, KIND_REPOSITORY_ERROR);
        assert!(
            report.items[0]
                .reasons
                .iter()
                .any(|reason| reason == "merged:error")
        );
        assert!(
            report.items[0]
                .error
                .as_deref()
                .unwrap()
                .contains("--merged-to")
        );
    }

    #[test]
    fn dry_run_reports_branch_delete_safety_error() {
        let fixture = GitFixture::new();
        fixture.create_branch("unmerged-dry-run");
        let worktree_path = fixture.temp.path().join("unmerged-dry-run-wt");
        fixture.add_worktree("unmerged-dry-run-wt", &worktree_path, "unmerged-dry-run");
        commit_in_worktree(&worktree_path, "dry-run.txt", "dry-run\n", "dry-run");

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            all: true,
            delete_branch: true,
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();
        let item = report
            .items
            .iter()
            .find(|item| item.path == worktree_path.display().to_string())
            .unwrap();

        assert_eq!(item.action, ACTION_WOULD_REMOVE);
        assert_eq!(
            item.branch_action.as_deref(),
            Some(crate::report::BRANCH_ACTION_ERROR)
        );
        assert!(item.branch_error.as_deref().unwrap().contains("not merged"));
        assert_eq!(report.summary.errors, 1);
        assert!(worktree_path.exists());
    }

    #[test]
    fn include_path_matches_worktree_basename() {
        let fixture = GitFixture::new();
        fixture.create_branch("basename-filter");
        let worktree_path = fixture.temp.path().join("basename-filter-wt");
        fixture.add_worktree("basename-filter-wt", &worktree_path, "basename-filter");

        let config = SweepConfig::from_args(crate::cli::Cli {
            paths: vec![fixture.repo_path.clone()],
            all: true,
            path_globs: vec!["basename-filter-wt".to_owned()],
            ..default_args()
        })
        .unwrap();
        let report = build_report(&config, &fixture.repo_path).unwrap();

        assert_eq!(report.summary.matched, 1);
        assert_eq!(report.items[0].path, worktree_path.display().to_string());
    }

    struct GitFixture {
        temp: TempDir,
        repo_path: std::path::PathBuf,
        repo: Repository,
    }

    impl GitFixture {
        fn new() -> Self {
            Self::new_with_initial_branch("main")
        }

        fn new_with_initial_branch(branch: &str) -> Self {
            let temp = tempfile::tempdir().unwrap();
            let repo_path = temp.path().join("repo");
            fs::create_dir(&repo_path).unwrap();
            let repo = Repository::init(&repo_path).unwrap();
            initial_commit(&repo, branch);
            Self {
                temp,
                repo_path,
                repo,
            }
        }

        fn create_branch(&self, name: &str) {
            let commit = self.repo.head().unwrap().peel_to_commit().unwrap();
            self.repo.branch(name, &commit, false).unwrap();
        }

        fn add_worktree(&self, name: &str, path: &Path, branch: &str) {
            let reference_name = format!("refs/heads/{branch}");
            let reference = self.repo.find_reference(&reference_name).unwrap();
            let mut options = git2::WorktreeAddOptions::new();
            options.reference(Some(&reference));
            self.repo.worktree(name, path, Some(&options)).unwrap();
        }

        fn set_missing_upstream(&self, branch: &str) {
            let mut config = self.repo.config().unwrap();
            config
                .set_str("remote.origin.url", "https://example.invalid/repo.git")
                .unwrap();
            config
                .set_str("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*")
                .unwrap();
            config
                .set_str(&format!("branch.{branch}.remote"), "origin")
                .unwrap();
            config
                .set_str(
                    &format!("branch.{branch}.merge"),
                    &format!("refs/heads/{branch}"),
                )
                .unwrap();
        }

        fn set_broken_origin_head(&self) {
            self.repo
                .reference_symbolic(
                    "refs/remotes/origin/HEAD",
                    "refs/remotes/origin/missing",
                    true,
                    "broken origin head",
                )
                .unwrap();
        }

        fn tag_branch_tip(&self, tag: &str, branch: &str) {
            let branch = self.repo.find_branch(branch, BranchType::Local).unwrap();
            let commit = branch.get().peel_to_commit().unwrap();
            self.repo
                .tag_lightweight(tag, commit.as_object(), false)
                .unwrap();
        }
    }

    #[derive(Default)]
    struct RecordingProgress {
        events: RefCell<Vec<String>>,
    }

    impl SweepProgress for RecordingProgress {
        fn begin_discovery(&self) {
            self.events.borrow_mut().push("begin_discovery".to_owned());
        }

        fn repositories_discovered(&self, total: usize) {
            self.events
                .borrow_mut()
                .push(format!("repositories_discovered:{total}"));
        }

        fn begin_repository(&self, path: &Path) {
            self.events
                .borrow_mut()
                .push(format!("begin_repository:{}", path.display()));
        }

        fn finish_repository(&self) {
            self.events
                .borrow_mut()
                .push("finish_repository".to_owned());
        }
    }

    impl DiscoveryProgress for RecordingProgress {
        fn directory_discovered(&self, path: &Path) {
            self.events
                .borrow_mut()
                .push(format!("directory_discovered:{}", path.display()));
        }

        fn directory_scanned(&self, path: &Path) {
            self.events
                .borrow_mut()
                .push(format!("directory_scanned:{}", path.display()));
        }
    }

    fn hidden_repo_with_merged_worktree() -> (TempDir, std::path::PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let hidden_dir = temp.path().join(".hidden");
        let repo_path = hidden_dir.join("repo");
        fs::create_dir_all(&repo_path).unwrap();
        let repo = Repository::init(&repo_path).unwrap();
        initial_commit(&repo, "main");
        let commit = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("merged-hidden", &commit, false).unwrap();
        let worktree_path = hidden_dir.join("merged-hidden-wt");
        let reference = repo.find_reference("refs/heads/merged-hidden").unwrap();
        let mut options = git2::WorktreeAddOptions::new();
        options.reference(Some(&reference));
        repo.worktree("merged-hidden-wt", &worktree_path, Some(&options))
            .unwrap();

        (temp, worktree_path)
    }

    fn initial_commit(repo: &Repository, branch: &str) {
        let signature = Signature::now("Test User", "test@example.com").unwrap();
        let workdir = repo.workdir().unwrap();
        repo.set_head(&format!("refs/heads/{branch}")).unwrap();
        fs::write(workdir.join("README.md"), "fixture\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("README.md")).unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
            .unwrap();
    }

    fn commit_in_worktree(path: &Path, file_name: &str, contents: &str, message: &str) {
        let repo = Repository::open(path).unwrap();
        let signature = Signature::now("Test User", "test@example.com").unwrap();
        fs::write(path.join(file_name), contents).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(file_name)).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &[&parent],
        )
        .unwrap();
    }
}
