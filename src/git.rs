use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use git2::{
    BranchType, ErrorCode, Oid, Repository, StatusOptions, StatusShow, WorktreeLockStatus,
    WorktreePruneOptions,
};

#[derive(Clone, Debug)]
pub struct RepoInfo {
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct DiscoveryError {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct DiscoveryResult {
    pub repos: Vec<RepoInfo>,
    pub errors: Vec<DiscoveryError>,
}

#[derive(Clone, Debug)]
pub struct MergeBase {
    pub display: String,
    pub oid: Oid,
    pub protected_branch_ref: Option<String>,
}

#[derive(Clone, Debug)]
pub struct WorktreeInfo {
    pub name: String,
    pub path: PathBuf,
    pub head: Option<Oid>,
    pub branch: Option<String>,
    pub locked: bool,
    pub prunable: bool,
    pub inspection_error: Option<String>,
}

pub trait DiscoveryProgress {
    fn directory_discovered(&self, _path: &Path) {}
    fn directory_scanned(&self, _path: &Path) {}
}

pub fn discover_repositories_with_progress(
    paths: &[PathBuf],
    recursive: bool,
    include_hidden: bool,
    progress: &impl DiscoveryProgress,
) -> DiscoveryResult {
    let mut candidates = Vec::new();
    let mut errors = Vec::new();

    for input in paths {
        let input = match input.canonicalize() {
            Ok(input) => input,
            Err(error) => {
                errors.push(DiscoveryError {
                    path: input.clone(),
                    message: format!("failed to resolve path {}: {error}", input.display()),
                });
                continue;
            }
        };
        match repo_root_for(&input) {
            Ok(Some(root)) => candidates.push(root),
            Ok(None) if recursive && input.is_dir() => {}
            Ok(None) => errors.push(DiscoveryError {
                path: input.clone(),
                message: format!("path is not inside a Git repository: {}", input.display()),
            }),
            Err(error) => errors.push(DiscoveryError {
                path: input.clone(),
                message: format!("{error:#}"),
            }),
        }

        if recursive && input.is_dir() {
            progress.directory_discovered(&input);
            discover_repositories_under(
                &input,
                include_hidden,
                progress,
                &mut candidates,
                &mut errors,
            );
        }
    }

    let mut seen = HashSet::new();
    let mut repos = Vec::new();
    for candidate in candidates {
        let repo = match open_repo(&candidate) {
            Ok(repo) => repo,
            Err(error) => {
                errors.push(DiscoveryError {
                    path: candidate,
                    message: format!("{error:#}"),
                });
                continue;
            }
        };
        let common_dir = canonicalize_best_effort(repo.commondir());
        if seen.insert(common_dir) {
            repos.push(RepoInfo {
                path: canonicalize_best_effort(&candidate),
            });
        }
    }

    DiscoveryResult { repos, errors }
}

pub fn open_repo(path: &Path) -> Result<Repository> {
    Repository::discover(path)
        .with_context(|| format!("failed to open Git repo at {}", path.display()))
}

pub fn current_worktree(path: &Path) -> Result<Option<PathBuf>> {
    let Some(root) = repo_root_for(path)? else {
        return Ok(None);
    };
    Ok(Some(canonicalize_best_effort(&root)))
}

pub fn resolve_merge_base(repo: &Repository, explicit: Option<&str>) -> Result<Option<MergeBase>> {
    if let Some(explicit) = explicit {
        let target = resolve_commit_target(repo, explicit)?
            .ok_or_else(|| anyhow!("merge target `{explicit}` was not found"))?;
        return Ok(Some(MergeBase {
            display: explicit.to_owned(),
            oid: target.oid,
            protected_branch_ref: target.protected_branch_ref,
        }));
    }

    if let Some(oid) = resolve_exact_ref_commit_oid(repo, "refs/remotes/origin/HEAD")? {
        return Ok(Some(MergeBase {
            display: "origin/HEAD".to_owned(),
            oid,
            protected_branch_ref: None,
        }));
    }

    for (display, reference) in [("main", "refs/heads/main"), ("master", "refs/heads/master")] {
        if let Some(oid) = resolve_exact_ref_commit_oid(repo, reference)? {
            return Ok(Some(MergeBase {
                display: display.to_owned(),
                oid,
                protected_branch_ref: Some(reference.to_owned()),
            }));
        }
    }

    Ok(None)
}

pub fn list_worktrees(repo: &Repository) -> Result<Vec<WorktreeInfo>> {
    let names = repo.worktrees().context("failed to list worktrees")?;
    let mut worktrees = Vec::new();

    for name in names.iter() {
        let Some(name) = name.context("failed to decode worktree name")? else {
            bail!("worktree list contains a non-UTF-8 name");
        };
        let worktree = repo
            .find_worktree(name)
            .with_context(|| format!("failed to open worktree `{name}`"))?;
        let path = canonicalize_best_effort(worktree.path());
        let locked = !matches!(worktree.is_locked()?, WorktreeLockStatus::Unlocked);
        let prunable = worktree.is_prunable(None)?;
        let (head, branch, inspection_error) = if prunable {
            (None, None, None)
        } else {
            match Repository::open_from_worktree(&worktree) {
                Ok(worktree_repo) => match head_and_branch(&worktree_repo) {
                    Ok((head, branch)) => (head, branch, None),
                    Err(error) => (None, None, Some(format!("{error:#}"))),
                },
                Err(error) => (
                    None,
                    None,
                    Some(format!(
                        "failed to open repository for worktree `{name}`: {error:#}"
                    )),
                ),
            }
        };

        worktrees.push(WorktreeInfo {
            name: name.to_owned(),
            path,
            head,
            branch,
            locked,
            prunable,
            inspection_error,
        });
    }

    Ok(worktrees)
}

pub fn branch_tracks_gone_upstream(repo: &Repository, branch: &str) -> Result<bool> {
    let refname = format!("refs/heads/{branch}");
    match repo.branch_upstream_name(&refname) {
        Ok(upstream_name) => {
            let upstream = upstream_name.as_str()?;
            match repo.find_reference(upstream) {
                Ok(_) => Ok(false),
                Err(error) if error.code() == ErrorCode::NotFound => Ok(true),
                Err(error) => Err(error.into()),
            }
        }
        Err(error) if error.code() == ErrorCode::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub fn is_ancestor(repo: &Repository, head: Oid, base: Oid) -> Result<bool> {
    if head == base {
        return Ok(true);
    }
    repo.graph_descendant_of(base, head)
        .context("failed to inspect commit ancestry")
}

pub fn is_worktree_dirty(path: &Path) -> Result<bool> {
    let repo = Repository::open(path)
        .with_context(|| format!("failed to open worktree at {}", path.display()))?;
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .show(StatusShow::IndexAndWorkdir);
    let statuses = repo
        .statuses(Some(&mut options))
        .with_context(|| format!("failed to read status for {}", path.display()))?;
    Ok(!statuses.is_empty())
}

pub fn remove_worktree(repo: &Repository, worktree_name: &str) -> Result<()> {
    let worktree = repo
        .find_worktree(worktree_name)
        .with_context(|| format!("failed to open worktree `{worktree_name}`"))?;
    let mut options = WorktreePruneOptions::new();
    options.valid(true).working_tree(true);
    worktree
        .prune(Some(&mut options))
        .with_context(|| format!("failed to remove worktree `{worktree_name}`"))
}

pub fn delete_branch_safely(
    repo: &Repository,
    branch_name: &str,
    merge_base: &MergeBase,
) -> Result<()> {
    check_branch_delete_safety(repo, branch_name, merge_base)?;
    let mut branch = repo
        .find_branch(branch_name, BranchType::Local)
        .with_context(|| format!("failed to open local branch `{branch_name}`"))?;
    branch
        .delete()
        .with_context(|| format!("failed to delete local branch `{branch_name}`"))
}

pub fn check_branch_delete_safety(
    repo: &Repository,
    branch_name: &str,
    merge_base: &MergeBase,
) -> Result<()> {
    let branch = repo
        .find_branch(branch_name, BranchType::Local)
        .with_context(|| format!("failed to open local branch `{branch_name}`"))?;
    let branch_refname = format!("refs/heads/{branch_name}");
    if merge_base.protected_branch_ref.as_deref() == Some(branch_refname.as_str()) {
        bail!("local branch `{branch_name}` is the merge target and cannot be deleted");
    }
    let branch_oid = branch
        .get()
        .peel_to_commit()
        .with_context(|| format!("failed to resolve local branch `{branch_name}`"))?
        .id();
    let base_oid = merge_base.oid;

    if !is_ancestor(repo, branch_oid, base_oid)? {
        bail!("local branch `{branch_name}` is not merged into the deletion base");
    }

    Ok(())
}

pub fn repo_root_for(path: &Path) -> Result<Option<PathBuf>> {
    let path = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };

    match Repository::discover(path) {
        Ok(repo) => Ok(repo.workdir().map(canonicalize_best_effort)),
        Err(error) if error.code() == ErrorCode::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {}", path.display())),
    }
}

pub fn canonicalize_best_effort(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub fn same_path(left: &Path, right: &Path) -> bool {
    canonicalize_best_effort(left) == canonicalize_best_effort(right)
}

fn discover_repositories_under(
    path: &Path,
    include_hidden: bool,
    progress: &impl DiscoveryProgress,
    candidates: &mut Vec<PathBuf>,
    errors: &mut Vec<DiscoveryError>,
) {
    progress.directory_scanned(path);

    if path.join(".git").exists() {
        candidates.push(path.to_path_buf());
    }

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => {
            errors.push(DiscoveryError {
                path: path.to_path_buf(),
                message: format!("failed to read directory {}: {error}", path.display()),
            });
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(DiscoveryError {
                    path: path.to_path_buf(),
                    message: format!(
                        "failed to read directory entry under {}: {error}",
                        path.display()
                    ),
                });
                continue;
            }
        };
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                errors.push(DiscoveryError {
                    path: entry.path(),
                    message: format!("failed to inspect file type: {error}"),
                });
                continue;
            }
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }

        let name = entry.file_name();
        if is_discovery_excluded(&name, include_hidden) {
            continue;
        }

        progress.directory_discovered(&entry.path());
        discover_repositories_under(&entry.path(), include_hidden, progress, candidates, errors);
    }
}

fn is_discovery_excluded(name: &std::ffi::OsStr, include_hidden: bool) -> bool {
    if name == std::ffi::OsStr::new(".git") {
        return true;
    }

    !include_hidden
        && name
            .to_str()
            .is_some_and(|name| name.starts_with('.') && name != ".")
}

struct CommitTarget {
    oid: Oid,
    protected_branch_ref: Option<String>,
}

fn resolve_commit_target(repo: &Repository, reference: &str) -> Result<Option<CommitTarget>> {
    match repo.revparse_ext(reference) {
        Ok((object, reference)) => {
            let protected_branch_ref = reference
                .and_then(|reference| reference.name().ok().map(ToOwned::to_owned))
                .filter(|name| name.starts_with("refs/heads/"));
            Ok(Some(CommitTarget {
                oid: object.peel_to_commit()?.id(),
                protected_branch_ref,
            }))
        }
        Err(error) if error.code() == ErrorCode::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn resolve_exact_ref_commit_oid(repo: &Repository, reference: &str) -> Result<Option<Oid>> {
    match repo.find_reference(reference) {
        Ok(reference) => match reference.peel_to_commit() {
            Ok(commit) => Ok(Some(commit.id())),
            Err(error) if matches!(error.code(), ErrorCode::NotFound | ErrorCode::UnbornBranch) => {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        },
        Err(error) if error.code() == ErrorCode::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn head_and_branch(repo: &Repository) -> Result<(Option<Oid>, Option<String>)> {
    let head = match repo.head() {
        Ok(head) => head,
        Err(error)
            if matches!(
                error.code(),
                ErrorCode::NotFound | ErrorCode::UnbornBranch | ErrorCode::Unmerged
            ) =>
        {
            return Ok((None, None));
        }
        Err(error) => return Err(error.into()),
    };

    let head_oid = head.peel_to_commit().ok().map(|commit| commit.id());
    let branch = if head.is_branch() {
        Some(head.shorthand()?.to_owned())
    } else {
        None
    };

    Ok((head_oid, branch))
}
