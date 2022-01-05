/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::symlink_metadata;
use std::path::Path;
use std::path::PathBuf;

use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use dashmap::DashMap;
use types::RepoPath;
use types::RepoPathBuf;

/// Audit repositories path to make sure that it is safe to write/remove through them.
///
/// This uses caching internally to avoid the heavy cost of querying the OS for each directory in
/// the path of a file.
///
/// The cache is concurrent and is shared between cloned instances of PathAuditor
pub struct PathAuditor {
    root: PathBuf,
    audited: DashMap<RepoPathBuf, ()>,
}

impl PathAuditor {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let audited = Default::default();
        let root = root.as_ref().to_owned();
        Self { root, audited }
    }

    /// Slow path, query the filesystem for unsupported path. Namely, writing through a symlink
    /// outside of the repo is not supported.
    /// XXX: more checks
    fn audit_fs(&self, path: &RepoPath) -> Result<()> {
        let full_path = self.root.join(path.as_str());

        // XXX: Maybe filter by specific errors?
        if let Ok(metadata) = symlink_metadata(&full_path) {
            ensure!(!metadata.file_type().is_symlink(), "{} is a symlink", path);
        }

        Ok(())
    }

    /// Make sure that it is safe to write/remove `path` from the repo.
    pub fn audit(&self, path: &RepoPath) -> Result<PathBuf> {
        for parent in path.parents() {
            // First fast check w/ read lock
            if !self.audited.contains_key(parent) {
                // If fast check failed, lock and do stat syscall
                // Alternatively we can just do syscall and then call insert
                self.audited
                    .entry(parent.to_owned())
                    .or_try_insert_with(|| {
                        if let Err(e) = self
                            .audit_fs(parent)
                            .with_context(|| format!("Can't audit path \"{}\"", parent))
                        {
                            Err(e)
                        } else {
                            Ok(())
                        }
                    })?;
            }
        }

        let mut filepath = self.root.to_owned();
        filepath.push(path.as_str());
        Ok(filepath)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::create_dir_all;
    use std::fs::read_link;
    use std::fs::remove_dir_all;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_audit_valid() -> Result<()> {
        let root = TempDir::new()?;

        let auditor = PathAuditor::new(&root);

        let repo_path = RepoPath::from_str("a/b")?;
        assert_eq!(
            auditor.audit(repo_path)?,
            root.as_ref().join(repo_path.as_str())
        );

        Ok(())
    }

    #[cfg(not(windows))]
    #[test]
    fn test_audit_invalid_symlink() -> Result<()> {
        let root = TempDir::new()?;
        let other = TempDir::new()?;

        let auditor = PathAuditor::new(&root);

        let link = root.as_ref().join("a");
        std::os::unix::fs::symlink(&other, &link)?;
        let canonical_other = other.as_ref().canonicalize()?;
        assert_eq!(read_link(&link)?.canonicalize()?, canonical_other);

        let repo_path = RepoPath::from_str("a/b")?;
        assert!(auditor.audit(repo_path).is_err());

        Ok(())
    }

    #[cfg(not(windows))]
    #[test]
    fn test_audit_caching() -> Result<()> {
        let root = TempDir::new()?;
        let other = TempDir::new()?;

        let path = root.as_ref().join("a");
        create_dir_all(&path)?;

        let auditor = PathAuditor::new(&root);

        // Populate the auditor cache.
        let repo_path = RepoPath::from_str("a/b")?;
        auditor.audit(&repo_path)?;

        remove_dir_all(&path)?;

        let link = root.as_ref().join("a");
        std::os::unix::fs::symlink(&other, &link)?;
        let canonical_other = other.as_ref().canonicalize()?;
        assert_eq!(read_link(&link)?.canonicalize()?, canonical_other);

        // Even though "a" is now a symlink to outside the repo, the audit will succeed due to the
        // one performed just above.
        let repo_path = RepoPath::from_str("a/b")?;
        auditor.audit(repo_path)?;

        Ok(())
    }
}
