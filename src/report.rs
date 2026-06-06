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

pub fn print_text_report(report: &SweepReport, verbose: bool) -> io::Result<()> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    write_text_report(&mut stdout, report, verbose)
}

fn write_text_report(
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

    let rows = items
        .iter()
        .map(|item| TextRow {
            item,
            action: item.action.as_str(),
            state: if item.dirty { "dirty" } else { "clean" },
            branch: item.branch.as_deref().unwrap_or("-"),
            reasons: item.reasons.join(","),
            path: item.path.as_str(),
        })
        .collect::<Vec<_>>();
    let action_width = column_width("ACTION", rows.iter().map(|row| row.action));
    let state_width = column_width("STATE", rows.iter().map(|row| row.state));
    let branch_width = column_width("BRANCH", rows.iter().map(|row| row.branch));
    let reasons_width = column_width("REASONS", rows.iter().map(|row| row.reasons.as_str()));

    writeln!(
        writer,
        "{:<action_width$}  {:<state_width$}  {:<branch_width$}  {:<reasons_width$}  PATH",
        "ACTION",
        "STATE",
        "BRANCH",
        "REASONS",
        action_width = action_width,
        state_width = state_width,
        branch_width = branch_width,
        reasons_width = reasons_width
    )?;
    for row in rows {
        writeln!(
            writer,
            "{:<action_width$}  {:<state_width$}  {:<branch_width$}  {:<reasons_width$}  {}",
            row.action,
            row.state,
            row.branch,
            row.reasons,
            row.path,
            action_width = action_width,
            state_width = state_width,
            branch_width = branch_width,
            reasons_width = reasons_width
        )?;

        if verbose {
            if let Some(reason) = &row.item.skip_reason {
                writeln!(writer, "  note: {reason}")?;
            }
            if let Some(error) = &row.item.error {
                writeln!(writer, "  error: {error}")?;
            }
            if let Some(action) = &row.item.branch_action {
                writeln!(writer, "  branch: {action}")?;
            }
            if let Some(error) = &row.item.branch_error {
                writeln!(writer, "  branch error: {error}")?;
            }
        }
    }

    if verbose {
        write_summary(writer, report)?;
    }

    Ok(())
}

struct TextRow<'a> {
    item: &'a SweepItem,
    action: &'a str,
    state: &'static str,
    branch: &'a str,
    reasons: String,
    path: &'a str,
}

fn column_width<'a>(header: &str, values: impl Iterator<Item = &'a str>) -> usize {
    values.fold(header.len(), |width, value| {
        width.max(value.chars().count())
    })
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
    fn text_report_is_silent_when_empty() {
        let report = SweepReport {
            items: Vec::new(),
            summary: SweepSummary::default(),
        };
        let mut output = Vec::new();

        write_text_report(&mut output, &report, false).unwrap();

        assert!(output.is_empty());
    }

    #[test]
    fn text_report_hides_errors_and_summary() {
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

        write_text_report(&mut output, &report, false).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("would_remove"));
        assert!(output.contains("clean"));
        assert!(output.contains("/repo-wt"));
        assert!(!output.contains("/bad"));
        assert!(!output.contains("Summary"));
        assert!(!output.contains("repo failed"));
        assert!(!output.contains("worktree detail"));
        assert!(!output.contains("branch detail"));
        assert!(!output.contains("skip note"));
    }

    #[test]
    fn text_report_uses_dynamic_column_widths() {
        let report = SweepReport {
            items: vec![
                SweepItem {
                    kind: KIND_WORKTREE.to_owned(),
                    repo: "/repo".to_owned(),
                    path: "/short".to_owned(),
                    branch: Some("a".to_owned()),
                    head: None,
                    reasons: vec!["gone".to_owned()],
                    dirty: false,
                    locked: false,
                    action: ACTION_WOULD_REMOVE.to_owned(),
                    skip_reason: None,
                    error: None,
                    branch_deleted: false,
                    branch_action: None,
                    branch_error: None,
                },
                SweepItem {
                    kind: KIND_WORKTREE.to_owned(),
                    repo: "/repo".to_owned(),
                    path: "/long".to_owned(),
                    branch: Some("long-feature-name".to_owned()),
                    head: None,
                    reasons: vec!["merged:main".to_owned(), "gone".to_owned()],
                    dirty: true,
                    locked: false,
                    action: ACTION_SKIP.to_owned(),
                    skip_reason: None,
                    error: None,
                    branch_deleted: false,
                    branch_action: None,
                    branch_error: None,
                },
            ],
            summary: SweepSummary::default(),
        };
        let mut output = Vec::new();

        write_text_report(&mut output, &report, false).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("ACTION        STATE  BRANCH             REASONS"));
        assert!(output.contains("would_remove  clean  a                  gone"));
        assert!(output.contains("skip          dirty  long-feature-name  merged:main,gone"));
    }

    #[test]
    fn verbose_text_report_includes_errors_and_summary() {
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

        write_text_report(&mut output, &report, true).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("repo:error"));
        assert!(output.contains("/bad"));
        assert!(output.contains("repo failed"));
        assert!(output.contains("Summary:"));
    }

    #[test]
    fn verbose_text_report_mentions_empty_results() {
        let report = SweepReport {
            items: Vec::new(),
            summary: SweepSummary {
                dry_run: true,
                repositories: 1,
                ..SweepSummary::default()
            },
        };
        let mut output = Vec::new();

        write_text_report(&mut output, &report, true).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("No matching worktrees."));
        assert!(output.contains("Summary:"));
    }
}
