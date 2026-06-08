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
    let now = SystemTime::now();
    Ok(!has_mtime_newer_than_age(path, now, age)?)
}

fn has_mtime_newer_than_age(path: &Path, now: SystemTime, age: Duration) -> std::io::Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    if mtime_is_newer_than_age(metadata.modified()?, now, age) {
        return Ok(true);
    }

    if metadata.is_dir() {
        return scan_has_mtime_newer_than_age(path, now, age);
    }

    Ok(false)
}

fn scan_has_mtime_newer_than_age(
    path: &Path,
    now: SystemTime,
    age: Duration,
) -> std::io::Result<bool> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_name = entry.file_name();
        if is_scan_excluded(&file_name) {
            continue;
        }

        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path)?;
        if mtime_is_newer_than_age(metadata.modified()?, now, age) {
            return Ok(true);
        }

        if metadata.is_dir() && scan_has_mtime_newer_than_age(&entry_path, now, age)? {
            return Ok(true);
        }
    }

    Ok(false)
}

fn mtime_is_newer_than_age(modified: SystemTime, now: SystemTime, age: Duration) -> bool {
    match now.duration_since(modified) {
        Ok(elapsed) => elapsed < age,
        Err(_) => true,
    }
}
