use std::io::{self, Write};

use serde::Serialize;

pub const ACTION_WOULD_REMOVE: &str = "would_remove";
pub const ACTION_REMOVED: &str = "removed";
pub const ACTION_SKIP: &str = "skip";
pub const ACTION_ERROR: &str = "error";
pub const BRANCH_ACTION_WOULD_DELETE: &str = "would_delete";
pub const BRANCH_ACTION_DELETED: &str = "deleted";
pub const BRANCH_ACTION_ERROR: &str = "error";
pub const KIND_WORKTREE: &str = "worktree";
pub const KIND_REPOSITORY_ERROR: &str = "repository_error";

#[derive(Debug, Serialize)]
pub struct SweepReport {
    pub items: Vec<SweepItem>,
    pub summary: SweepSummary,
}

#[derive(Debug, Default, Serialize)]
pub struct SweepSummary {
    pub dry_run: bool,
    pub repositories: usize,
    pub matched: usize,
    pub would_remove: usize,
    pub removed: usize,
    pub skipped: usize,
    pub errors: usize,
    pub branch_deleted: usize,
}

#[derive(Debug, Serialize)]
pub struct SweepItem {
    pub kind: String,
    pub repo: String,
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub reasons: Vec<String>,
    pub dirty: bool,
    pub locked: bool,
    pub action: String,
    pub skip_reason: Option<String>,
    pub error: Option<String>,
    pub branch_deleted: bool,
    pub branch_action: Option<String>,
    pub branch_error: Option<String>,
}

pub fn update_summary(summary: &mut SweepSummary, item: &SweepItem) {
    if item.kind == KIND_WORKTREE {
        summary.matched += 1;
    }

    match item.action.as_str() {
        ACTION_WOULD_REMOVE => summary.would_remove += 1,
        ACTION_REMOVED => summary.removed += 1,
        ACTION_SKIP => summary.skipped += 1,
        ACTION_ERROR => {}
        _ => {}
    }

    if item.action == ACTION_ERROR || item.error.is_some() || item.branch_error.is_some() {
        summary.errors += 1;
    }

    if item.branch_deleted {
        summary.branch_deleted += 1;
    }
}

pub fn print_human_report(report: &SweepReport, verbose: bool) -> io::Result<()> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    write_human_report(&mut stdout, report, verbose)
}

fn write_human_report(
    writer: &mut impl Write,
    report: &SweepReport,
    verbose: bool,
) -> io::Result<()> {
    let items = report
        .items
        .iter()
        .filter(|item| verbose || (item.kind == KIND_WORKTREE && item.action != ACTION_ERROR))
        .collect::<Vec<_>>();
    if items.is_empty() {
        if verbose {
            writeln!(writer, "No matching worktrees.")?;
            write_summary(writer, report)?;
        }
        return Ok(());
    }

    writeln!(
        writer,
        "{:<14} {:<5} {:<28} {:<22} PATH",
        "ACTION", "DIRTY", "BRANCH", "REASONS"
    )?;
    for item in items {
        let branch = item.branch.as_deref().unwrap_or("-");
        let reasons = item.reasons.join(",");
        writeln!(
            writer,
            "{:<14} {:<5} {:<28} {:<22} {}",
            item.action, item.dirty, branch, reasons, item.path
        )?;

        if verbose {
            if let Some(reason) = &item.skip_reason {
                writeln!(writer, "  note: {reason}")?;
            }
            if let Some(error) = &item.error {
                writeln!(writer, "  error: {error}")?;
            }
            if let Some(action) = &item.branch_action {
                writeln!(writer, "  branch: {action}")?;
            }
            if let Some(error) = &item.branch_error {
                writeln!(writer, "  branch error: {error}")?;
            }
        }
    }

    if verbose {
        write_summary(writer, report)?;
    }

    Ok(())
}

fn write_summary(writer: &mut impl Write, report: &SweepReport) -> io::Result<()> {
    writeln!(
        writer,
        "Summary: repositories={}, matched={}, would_remove={}, removed={}, skipped={}, errors={}, branch_deleted={}, dry_run={}",
        report.summary.repositories,
        report.summary.matched,
        report.summary.would_remove,
        report.summary.removed,
        report.summary.skipped,
        report.summary.errors,
        report.summary.branch_deleted,
        report.summary.dry_run
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_report_has_items_and_summary() {
        let report = SweepReport {
            items: vec![SweepItem {
                kind: KIND_WORKTREE.to_owned(),
                repo: "/repo".to_owned(),
                path: "/repo-wt".to_owned(),
                branch: Some("feature".to_owned()),
                head: Some("abc".to_owned()),
                reasons: vec!["gone".to_owned()],
                dirty: false,
                locked: false,
                action: ACTION_WOULD_REMOVE.to_owned(),
                skip_reason: None,
                error: None,
                branch_deleted: false,
                branch_action: None,
                branch_error: None,
            }],
            summary: SweepSummary {
                dry_run: true,
                repositories: 1,
                matched: 1,
                would_remove: 1,
                ..SweepSummary::default()
            },
        };

        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert_eq!(value["items"][0]["action"], ACTION_WOULD_REMOVE);
        assert_eq!(value["summary"]["dry_run"], true);
    }

    #[test]
    fn human_report_is_silent_when_empty() {
        let report = SweepReport {
            items: Vec::new(),
            summary: SweepSummary::default(),
        };
        let mut output = Vec::new();

        write_human_report(&mut output, &report, false).unwrap();

        assert!(output.is_empty());
    }

    #[test]
    fn human_report_hides_errors_and_summary() {
        let report = SweepReport {
            items: vec![
                SweepItem {
                    kind: KIND_REPOSITORY_ERROR.to_owned(),
                    repo: "/repo".to_owned(),
                    path: "/bad".to_owned(),
                    branch: None,
                    head: None,
                    reasons: vec!["repo:error".to_owned()],
                    dirty: false,
                    locked: false,
                    action: ACTION_ERROR.to_owned(),
                    skip_reason: None,
                    error: Some("repo failed".to_owned()),
                    branch_deleted: false,
                    branch_action: None,
                    branch_error: None,
                },
                SweepItem {
                    kind: KIND_WORKTREE.to_owned(),
                    repo: "/repo".to_owned(),
                    path: "/repo-wt".to_owned(),
                    branch: Some("feature".to_owned()),
                    head: Some("abc".to_owned()),
                    reasons: vec!["gone".to_owned()],
                    dirty: false,
                    locked: false,
                    action: ACTION_WOULD_REMOVE.to_owned(),
                    skip_reason: Some("skip note".to_owned()),
                    error: Some("worktree detail".to_owned()),
                    branch_deleted: false,
                    branch_action: Some(BRANCH_ACTION_ERROR.to_owned()),
                    branch_error: Some("branch detail".to_owned()),
                },
            ],
            summary: SweepSummary {
                dry_run: true,
                repositories: 1,
                matched: 1,
                would_remove: 1,
                errors: 2,
                ..SweepSummary::default()
            },
        };
        let mut output = Vec::new();

        write_human_report(&mut output, &report, false).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("would_remove"));
        assert!(output.contains("/repo-wt"));
        assert!(!output.contains("/bad"));
        assert!(!output.contains("Summary"));
        assert!(!output.contains("repo failed"));
        assert!(!output.contains("worktree detail"));
        assert!(!output.contains("branch detail"));
        assert!(!output.contains("skip note"));
    }

    #[test]
    fn verbose_human_report_includes_errors_and_summary() {
        let report = SweepReport {
            items: vec![SweepItem {
                kind: KIND_REPOSITORY_ERROR.to_owned(),
                repo: "/repo".to_owned(),
                path: "/bad".to_owned(),
                branch: None,
                head: None,
                reasons: vec!["repo:error".to_owned()],
                dirty: false,
                locked: false,
                action: ACTION_ERROR.to_owned(),
                skip_reason: None,
                error: Some("repo failed".to_owned()),
                branch_deleted: false,
                branch_action: None,
                branch_error: None,
            }],
            summary: SweepSummary {
                dry_run: true,
                repositories: 0,
                errors: 1,
                ..SweepSummary::default()
            },
        };
        let mut output = Vec::new();

        write_human_report(&mut output, &report, true).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("repo:error"));
        assert!(output.contains("/bad"));
        assert!(output.contains("repo failed"));
        assert!(output.contains("Summary:"));
    }

    #[test]
    fn verbose_human_report_mentions_empty_results() {
        let report = SweepReport {
            items: Vec::new(),
            summary: SweepSummary {
                dry_run: true,
                repositories: 1,
                ..SweepSummary::default()
            },
        };
        let mut output = Vec::new();

        write_human_report(&mut output, &report, true).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("No matching worktrees."));
        assert!(output.contains("Summary:"));
    }
}
