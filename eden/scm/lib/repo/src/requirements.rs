/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use fs_err as fs;
use util::errors::IOContext;

use crate::errors::RequirementsOpenError;
use crate::errors::UnsupportedRequirements;

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
    pub fn open(path: &Path, supported: &HashSet<String>) -> Result<Self, RequirementsOpenError> {
        let requirements: HashSet<String> = match fs::read_to_string(path) {
            Ok(s) => s.split_whitespace().map(|s| s.to_string()).collect(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Default::default(),
            Err(e) => {
                return Err(RequirementsOpenError::IOError(
                    Err::<Self, std::io::Error>(e)
                        .path_context("error opening file", path)
                        .unwrap_err(),
                ));
            }
        };
        let mut unsupported = requirements.difference(supported).peekable();
        if unsupported.peek().is_some() {
            let mut unsupported_vec: Vec<_> = unsupported.cloned().collect();
            unsupported_vec.sort();
            return Err(RequirementsOpenError::UnsupportedRequirements(
                UnsupportedRequirements(unsupported_vec.join(", ")),
            ));
        }
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
    pub fn flush(&mut self) -> io::Result<()> {
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

    /// Clone as a `HashSet`.
    pub fn to_set(&self) -> HashSet<String> {
        self.requirements.clone()
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_requires_basic() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("requires");
        let allowed = HashSet::<String>::from(["a".to_owned(), "b".to_owned(), "c".to_owned()]);
        let mut reqs = Requirements::open(&path, &allowed).unwrap();

        assert!(!reqs.contains("a"));
        assert!(!reqs.contains("b"));

        reqs.add("a");
        assert!(reqs.contains("a"));
        assert!(!reqs.contains("b"));

        // add() buffers changes.
        let reqs2 = Requirements::open(&path, &allowed).unwrap();
        assert!(!reqs2.contains("a"));

        // add() again is not an error.
        reqs.add("a");
        reqs.add("b");

        // flush() writes changes.
        reqs.flush().unwrap();
        let reqs2 = Requirements::open(&path, &allowed).unwrap();
        assert!(reqs2.contains("a"));
        assert!(reqs2.contains("b"));
        assert!(!reqs2.contains("c"));

        assert!(reqs.contains("a"));
        assert!(reqs.contains("b"));
        assert!(!reqs.contains("c"));
    }

    #[test]
    fn test_unallowed_requirements() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("requires");
        let allowed = HashSet::<String>::from(["a".to_owned()]);
        let mut reqs = Requirements::open(&path, &allowed).unwrap();
        reqs.add("foo");
        reqs.add("bar");
        reqs.flush().unwrap();
        let err = Requirements::open(&path, &allowed).err().unwrap();
        assert!(matches!(
            err,
            RequirementsOpenError::UnsupportedRequirements(_)
        ));
        assert_eq!(
            err.to_string(),
            r#"repository requires unknown features: bar, foo
(see https://mercurial-scm.org/wiki/MissingRequirement for more information)"#
        );
    }
}
