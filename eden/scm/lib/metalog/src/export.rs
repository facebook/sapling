/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::metalog::load_root;
use crate::MetaLog;
use crate::Result;
use git2::Repository;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

impl MetaLog {
    /// Export metalog to a git repo for investigation.
    pub fn export_git(&self, repo_path: &Path) -> Result<()> {
        let repo = Repository::init(repo_path)?;
        let root_ids = Self::list_roots(&self.path)?;

        // We use a packfile per key for efficient deltaing.
        let mut blob_id_map = HashMap::new();
        let mut last_commit = None;
        let mut count = 0;
        for root_id in root_ids {
            // Stop at the current RootId.
            if root_id == self.orig_root_id {
                break;
            }

            let root = load_root(&self.blobs, root_id)?;

            // Add blobs.
            for (_key, value_id) in root.map.iter() {
                if blob_id_map.contains_key(value_id) {
                    continue;
                }
                let mut writer = repo.blob_writer(None)?;
                let value = self
                    .blobs
                    .get(*value_id)?
                    .ok_or_else(|| self.error(format!("cannot read {:?}", value_id)))?;
                writer.write_all(&value)?;

                let git_blob_id = writer.commit()?;
                blob_id_map.insert(*value_id, git_blob_id);
            }

            // Add tree.
            let mut tree = repo.treebuilder(None)?;
            for (key, value_id) in root.map.iter() {
                let git_blob_id = blob_id_map.get(value_id).unwrap();
                tree.insert(key, *git_blob_id, 0o100644)?;
            }
            let tree_id = tree.write()?;

            // Add commit.
            let time = git2::Time::new(root.timestamp as _, 0);
            let sig = git2::Signature::new("metalog", "metalog@example.com", &time)?;
            let parents = match last_commit {
                None => Vec::new(),
                Some(ref parent) => vec![parent],
            };
            let message = format!("{}\n\nRootId: {}", root.message, root_id.to_hex());
            let tree = repo.find_tree(tree_id)?;
            last_commit =
                Some(repo.find_commit(repo.commit(None, &sig, &sig, &message, &tree, &parents)?)?);
            count += 1;
            if count % 100 == 0 {
                eprintln!("count: {}", count);
            }
        }

        // Make 'master' point to the last commit.
        if let Some(last_commit) = last_commit {
            repo.reference(
                "refs/heads/master",
                last_commit.id(),
                true, /* force */
                "move master",
            )?;
        }

        Ok(())
    }
}
