/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

use git2::Repository;

use crate::metalog::load_root;
use crate::metalog::Id20;
use crate::metalog::SerId20;
use crate::MetaLog;
use crate::Result;

impl MetaLog {
    /// Export metalog to a git repo for investigation.
    pub fn export_git(&self, repo_path: &Path) -> Result<()> {
        let repo = Repository::init(repo_path)?;
        let root_ids = Self::list_roots(&self.path)?;

        let mut blob_id_map = HashMap::new(); // Metalog Blob SHA1 -> Git Blob SHA1
        let mut commit_id_map = HashMap::new(); // Metalog Root SHA1 -> Git Commit
        let listed: HashSet<_> = root_ids.iter().copied().collect();
        let mut count = 0;

        // Figure out the "parents" relationship.
        let parents: HashMap<Id20, Vec<Id20>> = {
            let mut parents: HashMap<Id20, Vec<Id20>> = HashMap::new();

            // From the root_id list.
            for slice in root_ids.windows(2) {
                if let [parent, child] = slice {
                    parents.insert(*child, vec![*parent]);
                }
            }

            // From the implicit "Parent: " messages.
            // They might include pending changes. See D30970502.
            for root_id in root_ids.iter().copied() {
                let root = load_root(&self.blobs.read(), root_id)?;
                for line in root.message.lines() {
                    if let Some(hex_parent) = line.strip_prefix("Parent: ") {
                        if let Ok(parent_root_id) = Id20::from_hex(hex_parent.as_bytes()) {
                            if let Ok(_parent_root) = load_root(&self.blobs.read(), parent_root_id)
                            {
                                let parents = parents.entry(root_id).or_default();
                                if !parents.contains(&parent_root_id) {
                                    parents.push(parent_root_id);
                                }
                            }
                        }
                    }
                }
            }
            parents
        };

        // Export everything reachable from the "current" root.
        let mut to_visit: Vec<Id20> = vec![self.orig_root_id];
        while let Some(root_id) = to_visit.pop() {
            if commit_id_map.contains_key(&root_id) {
                // Already committed.
                continue;
            }

            let root_parents: &[Id20] = match parents.get(&root_id) {
                Some(parents) => parents.as_ref(),
                None => &[],
            };

            {
                // Need to commit missing parents first?
                let mut missing_parents = false;
                for parent in root_parents {
                    if !commit_id_map.contains_key(parent) {
                        if !missing_parents {
                            to_visit.push(root_id);
                            missing_parents = true;
                        }
                        to_visit.push(*parent);
                    }
                }
                if missing_parents {
                    continue;
                }
            }

            let root = load_root(&self.blobs.read(), root_id)?;

            // Add blobs.
            for (_key, SerId20(value_id)) in root.map.iter() {
                if blob_id_map.contains_key(value_id) {
                    continue;
                }
                let mut writer = repo.blob_writer(None)?;
                let value = self
                    .blobs
                    .read()
                    .get(*value_id)?
                    .ok_or_else(|| self.error(format!("cannot read {:?}", value_id)))?;
                writer.write_all(&value)?;

                let git_blob_id = writer.commit()?;
                blob_id_map.insert(*value_id, git_blob_id);
            }

            // Add tree.
            let mut tree = repo.treebuilder(None)?;
            for (key, SerId20(value_id)) in root.map.iter() {
                let git_blob_id = blob_id_map.get(value_id).unwrap();
                tree.insert(key, *git_blob_id, 0o100644)?;
            }
            let tree_id = tree.write()?;

            // Add commit.
            let time = git2::Time::new(root.timestamp as _, 0);
            let sig = git2::Signature::new("metalog", "metalog@example.com", &time)?;
            let git_parents = root_parents
                .iter()
                .filter_map(|p| commit_id_map.get(p))
                .collect::<Vec<_>>();
            let detach_message = if listed.contains(&root_id) {
                ""
            } else {
                "\nDetached: true"
            };
            let message = format!(
                "{}\n\nRootId: {}{}",
                root.message,
                root_id.to_hex(),
                detach_message
            );
            let tree = repo.find_tree(tree_id)?;
            let commit_oid = repo.commit(None, &sig, &sig, &message, &tree, &git_parents)?;
            let commit = repo.find_commit(commit_oid)?;
            commit_id_map.insert(root_id, commit);
            count += 1;
            if count % 100 == 0 {
                eprintln!("count: {}", count);
            }
        }

        // Make 'master' point to the last commit.
        if let Some(main_commit) = commit_id_map.get(&self.orig_root_id) {
            repo.reference(
                "refs/heads/master",
                main_commit.id(),
                true, /* force */
                "move master",
            )?;
        }

        Ok(())
    }
}
