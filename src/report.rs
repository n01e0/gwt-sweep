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

pub fn print_human_report(report: &SweepReport) {
    if report.items.is_empty() {
        println!("No matching worktrees.");
        println!(
            "Summary: repositories={}, dry_run={}",
            report.summary.repositories, report.summary.dry_run
        );
        return;
    }

    println!(
        "{:<14} {:<5} {:<28} {:<22} PATH",
        "ACTION", "DIRTY", "BRANCH", "REASONS"
    );
    for item in &report.items {
        let branch = item.branch.as_deref().unwrap_or("-");
        let reasons = item.reasons.join(",");
        println!(
            "{:<14} {:<5} {:<28} {:<22} {}",
            item.action, item.dirty, branch, reasons, item.path
        );

        if let Some(reason) = &item.skip_reason {
            println!("  note: {reason}");
        }
        if let Some(error) = &item.error {
            println!("  error: {error}");
        }
        if let Some(action) = &item.branch_action {
            println!("  branch: {action}");
        }
        if let Some(error) = &item.branch_error {
            println!("  branch error: {error}");
        }
    }

    println!(
        "Summary: repositories={}, matched={}, would_remove={}, removed={}, skipped={}, errors={}, branch_deleted={}, dry_run={}",
        report.summary.repositories,
        report.summary.matched,
        report.summary.would_remove,
        report.summary.removed,
        report.summary.skipped,
        report.summary.errors,
        report.summary.branch_deleted,
        report.summary.dry_run
    );
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
}
