/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::Result;
use dag::ops::DagPersistent;
use dag::Dag;
use metalog::CommitOptions;
use metalog::MetaLog;
use minibytes::Bytes;
use std::path::Path;
use zstore::Id20;
use zstore::Zstore;

/// Non-lazy, pure Rust, local repo implementation.
///
/// Mainly useful as a simple "server repo" in tests that can replace ssh remote
/// repos and exercise EdenApi features.
///
/// Format-wise, an eager repo includes:
///
/// ## SHA1 Key/Value Content Store
///
/// File, tree, commit contents.
///
/// SHA1 is verifiable. For HG this means `sorted([p1, p2])` and filelog rename
/// metadata is included in values.
///
/// This is meant to be mainly a content store. We currently "abuse" it to
/// answer filelog history. The filelog (filenode) and linknodes are
/// considered tech-debt and we hope to replace them with fastlog APIs which
/// serve sub-graph with `(commit, path)` as graph nodes.
///
/// We don't use `(p1, p2)` for commit parents because it loses the parent
/// order. The DAG storage is used to answer commit parents instead.
///
/// Currently backed by [`zstore::Zstore`]. For simplicity, we don't use the
/// zstore delta-compress features, and don't store different types separately.
///
///
/// ## Commit Graph
///
/// Commit hashes and parent commit hashes.
///
/// Currently backed by the [`dag::Dag`]. It handles the main complexity.
///
///
/// ## Metadata
///
/// Bookmarks, tip, remote bookmarks, visible heads, etc.
///
/// Format is made compatible with the Python code. Only bookmarks is
/// implemented for now to support testing use-cases.
///
/// Currently backed by [`metalog::MetaLog`]. It's a lightweight source control
/// for atomic metadata changes.
pub struct EagerRepo {
    dag: Dag,
    store: Zstore,
    metalog: MetaLog,
}

impl EagerRepo {
    /// Open an [`EagerRepo`] at the given directory. Create an empty repo on demand.
    pub fn open(dir: &Path) -> Result<Self> {
        // Attempt to match directory layout of a real client repo.
        let dir = dir.join(".hg/store");
        let dag = Dag::open(dir.join("segments/v1"))?;
        let store = Zstore::open(dir.join("hgcommits/v1"))?;
        let metalog = MetaLog::open(dir.join("metalog"), None)?;
        let repo = Self {
            dag,
            store,
            metalog,
        };
        Ok(repo)
    }

    /// Write pending changes to disk.
    pub async fn flush(&mut self) -> Result<()> {
        self.store.flush()?;
        self.dag.flush(&[]).await?;
        let opts = CommitOptions::default();
        self.metalog.commit(opts)?;
        Ok(())
    }

    // The following APIs provide low-level ways to read or write the repo.
    //
    // They are used for push before EdenApi provides push related APIs.

    /// Insert SHA1 blob to zstore.
    /// In hg's case, the `data` is `min(p1, p2) + max(p1, p2) + text`.
    pub fn add_sha1_blob(&mut self, data: &[u8]) -> Result<Id20> {
        // SPACE: This does not utilize zstore's delta features to save space.
        Ok(self.store.insert(data, &[])?)
    }

    /// Read SHA1 blob from zstore.
    pub fn get_sha1_blob(&self, id: Id20) -> Result<Option<Bytes>> {
        Ok(self.store.get(id)?)
    }

    /// Obtain a reference to the commit graph.
    pub fn dag(&self) -> &Dag {
        &self.dag
    }

    /// Obtain a reference to the metalog.
    pub fn metalog(&self) -> &MetaLog {
        &self.metalog
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_read_write_blob() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();

        let mut repo = EagerRepo::open(dir).unwrap();
        let text = &b"blob-text-foo-bar"[..];
        let id = repo.add_sha1_blob(text).unwrap();
        assert_eq!(repo.get_sha1_blob(id).unwrap().as_deref(), Some(text));

        // Pending changes are invisible until flush.
        let repo2 = EagerRepo::open(dir).unwrap();
        assert!(repo2.get_sha1_blob(id).unwrap().is_none());

        repo.flush().await.unwrap();

        let repo2 = EagerRepo::open(dir).unwrap();
        assert_eq!(repo2.get_sha1_blob(id).unwrap().as_deref(), Some(text));
    }
}
