use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

const EXCLUDED_SCAN_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".venv",
    "venv",
    "dist",
    "build",
    ".cache",
];

pub fn is_scan_excluded(name: &OsStr) -> bool {
    EXCLUDED_SCAN_DIRS
        .iter()
        .any(|excluded| name == OsStr::new(excluded))
}

pub fn is_older_than(path: &Path, age: Duration) -> std::io::Result<bool> {
    let latest = latest_mtime(path)?;
    match SystemTime::now().duration_since(latest) {
        Ok(elapsed) => Ok(elapsed >= age),
        Err(_) => Ok(false),
    }
}

fn latest_mtime(path: &Path) -> std::io::Result<SystemTime> {
    let metadata = fs::symlink_metadata(path)?;
    let mut latest = metadata.modified()?;

    if metadata.is_dir() {
        scan_latest_mtime(path, &mut latest)?;
    }

    Ok(latest)
}

fn scan_latest_mtime(path: &Path, latest: &mut SystemTime) -> std::io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_name = entry.file_name();
        if is_scan_excluded(&file_name) {
            continue;
        }

        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path)?;
        let modified = metadata.modified()?;
        if modified > *latest {
            *latest = modified;
        }

        if metadata.is_dir() {
            scan_latest_mtime(&entry_path, latest)?;
        }
    }
    Ok(())
}
