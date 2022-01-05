/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Simple crate to call fsync on files matching glob patterns.
//!
//! This is a standalone crate to help reducing compile time of `hgcommands`.

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use glob::Pattern;
use tracing::debug;
use tracing::trace;
use tracing::warn;

/// Call `fsync` on files matching given glob patterns under the given directory.
///
/// Errors are silenced and logged to tracing framework.
/// Files not recently modified (older than `newer_than`) are skipped.
///
/// Returns paths that are fsync-ed.
pub fn fsync_glob(dir: &Path, patterns: &[&str], newer_than: Option<SystemTime>) -> Vec<PathBuf> {
    let escaped_dir = Pattern::escape(&dir.display().to_string());
    let mut result = Vec::new();
    for p in patterns {
        let full_pattern = format!("{}/{}", &escaped_dir, p);
        debug!("globing {}", &full_pattern);

        let matches = match glob::glob(&full_pattern) {
            Err(e) => {
                warn!("glob failed: {}", e);
                continue;
            }
            Ok(matches) => matches,
        };

        let newer_than = newer_than.unwrap_or_else(|| {
            let now = SystemTime::now();
            now.checked_sub(Duration::from_secs(300)).unwrap_or(now)
        });

        for path in matches {
            let path = match path {
                Ok(path) => path,
                Err(e) => {
                    warn!("path reading failed: {}", e);
                    continue;
                }
            };

            match try_fsync_if_newer_than(&path, newer_than) {
                Ok(true) => {
                    if let Ok(path) = path.strip_prefix(dir) {
                        result.push(path.to_path_buf());
                    }
                    debug!("fsynced: {}", path.display());
                }
                Ok(false) => trace!("skipped: {}", path.display()),
                Err(e) => warn!("cannot fsync {}: {}", path.display(), e),
            }
        }
    }
    result.sort_unstable();

    // Also fsync parent directories on *nix. This syncs metadata about the file.
    // On Windows directories cannot be opened.
    #[cfg(unix)]
    {
        let dirs: Vec<&Path> = {
            let mut dirs: Vec<_> = result.iter().filter_map(|p| p.parent()).collect();
            dirs.dedup();
            dirs
        };
        for path in dirs {
            let path = dir.join(path);
            match fs::OpenOptions::new().read(true).open(&path) {
                Ok(file) => match file.sync_all() {
                    Ok(_) => debug!("fsynced dir: {}", path.display()),
                    Err(e) => warn!("cannot fsync dir {}: {}", path.display(), e),
                },
                Err(e) => warn!("cannot open dir {}: {}", path.display(), e),
            }
        }
    }
    result
}

/// Attempt to fsync a single file.
/// Return false if the file is skipped (not newly modified or not a file).
/// Return true if the file is synced.
fn try_fsync_if_newer_than(path: &Path, newer_than: SystemTime) -> io::Result<bool> {
    let metadata = path.symlink_metadata()?;
    if !metadata.is_file() || metadata.modified()? < newer_than {
        return Ok(false);
    }

    let mut open_opts = fs::OpenOptions::new();
    open_opts.read(true).create(false).truncate(false);

    // Windows requires opening with write permission for fsync.
    if cfg!(windows) {
        open_opts.write(true);
    }

    let file = open_opts.open(path)?;
    file.sync_all()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_patterns() {
        let dir = tempdir().unwrap();
        let dir = dir.path();

        fs::write(dir.join("a"), b"1").unwrap();
        fs::write(dir.join("a1"), b"1").unwrap();
        fs::write(dir.join("b"), b"2").unwrap();
        fs::write(dir.join("c"), b"3").unwrap();

        assert_eq!(d(fsync_glob(&dir, &[], None)), "[]");
        assert_eq!(d(fsync_glob(&dir, &["d"], None)), "[]");
        assert_eq!(d(fsync_glob(&dir, &["?"], None)), "[\"a\", \"b\", \"c\"]");
        assert_eq!(
            d(fsync_glob(&dir, &["a*", "c"], None)),
            "[\"a\", \"a1\", \"c\"]"
        );
    }

    #[test]
    fn test_skip_old_files() {
        let dir = tempdir().unwrap();
        let dir = dir.path();

        fs::write(dir.join("a"), b"1").unwrap();
        fs::write(dir.join("b"), b"2").unwrap();

        let newer_than = SystemTime::now()
            .checked_add(Duration::from_secs(10))
            .unwrap();
        assert_eq!(d(fsync_glob(&dir, &["*"], Some(newer_than))), "[]");
    }

    fn d(value: impl std::fmt::Debug) -> String {
        format!("{:?}", value)
    }
}
