/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use util::errors::IOContext;
use util::errors::IOResult;

/// `Requirements` contains a set of strings, tracks features/capabilities that
/// are *required* for clients to interface with the repository.
///
/// Requirements are often associated with actual on-disk formats. Different
/// requirements might require different code paths to process the on-disk
/// structure.
#[derive(Debug, Clone)]
pub struct Requirements {
    path: PathBuf,
    requirements: HashSet<String>,
    dirty: bool,
}

impl Requirements {
    /// Load requirements from the given path.
    ///
    /// If the given path does not exist, it is treated as an empty file.
    pub fn open(path: &Path) -> IOResult<Self> {
        let requirements = match fs::read_to_string(path) {
            Ok(s) => s.split_whitespace().map(|s| s.to_string()).collect(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Default::default(),
            Err(e) => return Err(e).path_context("error opening file", path),
        };
        let path = path.to_path_buf();
        let dirty = false;
        Ok(Self {
            path,
            requirements,
            dirty,
        })
    }

    /// Returns true if the given feature is enabled.
    pub fn contains(&self, name: &str) -> bool {
        self.requirements.contains(name)
    }

    /// Add a requirement. It is buffered in memory until `flush()`.
    ///
    /// This is usually part of a complex operation and protected by a
    /// filesystem lock.
    pub fn add(&mut self, name: &str) {
        let inserted = self.requirements.insert(name.to_string());
        self.dirty = self.dirty || inserted;
    }

    /// Write requirement changes to disk.
    ///
    /// This is usually part of a complex operation and protected by a
    /// filesystem lock.
    pub fn flush(&mut self) -> IOResult<()> {
        if self.dirty {
            util::file::atomic_write(&self.path, |f| {
                let mut requires: Vec<&str> =
                    self.requirements.iter().map(|s| s.as_str()).collect();
                requires.sort_unstable();
                let mut text = requires.join("\n");
                if !text.is_empty() {
                    text.push('\n');
                }
                f.write_all(text.as_bytes())
            })?;
            self.dirty = false;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_requires_basic() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("requires");
        let mut reqs = Requirements::open(&path).unwrap();

        assert!(!reqs.contains("a"));
        assert!(!reqs.contains("b"));

        reqs.add("a");
        assert!(reqs.contains("a"));
        assert!(!reqs.contains("b"));

        // add() buffers changes.
        let reqs2 = Requirements::open(&path).unwrap();
        assert!(!reqs2.contains("a"));

        // add() again is not an error.
        reqs.add("a");
        reqs.add("b");

        // flush() writes changes.
        reqs.flush().unwrap();
        let reqs2 = Requirements::open(&path).unwrap();
        assert!(reqs2.contains("a"));
        assert!(reqs2.contains("b"));
        assert!(!reqs2.contains("c"));

        assert!(reqs.contains("a"));
        assert!(reqs.contains("b"));
        assert!(!reqs.contains("c"));
    }
}
