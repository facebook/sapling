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
use std::process::Command;
use std::process::Stdio;

use crate::metalog::load_root;
use crate::metalog::Id20;
use crate::metalog::SerId20;
use crate::MetaLog;
use crate::Result;

impl MetaLog {
    /// Export metalog to a git repo for investigation.
    pub fn export_git(&self, repo_path: &Path) -> Result<()> {
        let mut payload = FastImportPayload::new("metalog <metalog@example.com>");

        let root_ids = Self::list_roots(&self.path)?;
        let mut blob_id_map = HashMap::new(); // Metalog Blob SHA1 -> BlobId
        let mut commit_id_map = HashMap::new(); // Metalog Root SHA1 -> CommitId
        let listed: HashSet<_> = root_ids.iter().copied().collect();

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
                let value = self
                    .blobs
                    .read()
                    .get(*value_id)?
                    .ok_or_else(|| self.error(format!("cannot read {:?}", value_id)))?;
                let git_blob_id = payload.blob(&value);
                blob_id_map.insert(*value_id, git_blob_id);
            }

            // Prepare files.
            let path_blob_ids: Vec<(String, BlobId)> = root
                .map
                .iter()
                .map(|(path, SerId20(value_id))| (path.to_string(), blob_id_map[value_id]))
                .collect();

            // Add commit.
            let git_parents: Vec<CommitId> = root_parents
                .iter()
                .filter_map(|p| commit_id_map.get(p))
                .cloned()
                .collect();
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
            let commit_id = payload.commit(&message, root.timestamp, &git_parents, &path_blob_ids);
            commit_id_map.insert(root_id, commit_id);
        }

        // Run 'git init' if the directory does not exist.
        if !repo_path.is_dir() {
            Command::new("git")
                .args(["-c", "init.defaultBranch=main", "init", "-q"])
                .arg(repo_path)
                .status()?;
        }

        // Run 'git fast-import'.
        let payload: Vec<u8> = payload.into_vec();
        let mut import_process = Command::new("git")
            .args(["fast-import", "--quiet"])
            .current_dir(repo_path)
            .stdin(Stdio::piped())
            .spawn()?;
        let mut stdin = import_process.stdin.take().unwrap();
        stdin.write_all(&payload)?;
        drop(stdin);

        import_process.wait()?;

        Ok(())
    }
}

/// Payload for git-fast-import.
struct FastImportPayload {
    author: &'static str,
    id_count: usize,
    payload: Vec<u8>,
}

#[derive(Copy, Clone, Debug)]
struct BlobId(usize);

#[derive(Copy, Clone, Debug)]
struct CommitId(usize);

impl FastImportPayload {
    pub fn new(author: &'static str) -> Self {
        Self {
            author,
            id_count: 0,
            payload: Vec::new(),
        }
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.payload
    }

    pub fn blob(&mut self, data: &[u8]) -> BlobId {
        let id = self.next_id();
        let payload = &mut self.payload;
        payload.extend_from_slice(b"blob\n");
        payload.extend_from_slice(format!("mark :{}\n", id).as_bytes());
        payload.extend_from_slice(format!("data {}\n", data.len()).as_bytes());
        payload.extend_from_slice(data);
        payload.push(b'\n');
        BlobId(id)
    }

    pub fn commit(
        &mut self,
        message: &str,
        timestamp: u64,
        parents: &[CommitId],
        path_blob_ids: &[(String, BlobId)],
    ) -> CommitId {
        let commit_id = self.next_id();
        let parents_lines = parents
            .iter()
            .enumerate()
            .map(|(i, parent_id)| {
                let prefix = if i == 0 { "from" } else { "merge" };
                format!("{} :{}\n", prefix, parent_id.0)
            })
            .collect::<Vec<_>>()
            .concat();
        let files_lines = path_blob_ids
            .iter()
            .map(|(path, blob_id)| format!("M 100644 :{} {}\n", blob_id.0, path))
            .collect::<Vec<_>>()
            .concat();
        let when = format!("{} +0000", timestamp);
        let commit_payload = format!(
            concat!(
                "commit refs/heads/main\n",
                "mark :{commit_id}\n",
                "committer {author} {when}\n",
                "data {message_len}\n{message}\n",
                "{parents_lines}",
                "deleteall\n",
                "{files_lines}",
                "\n",
            ),
            commit_id = commit_id,
            author = self.author,
            when = when,
            message_len = message.len(),
            message = &message,
            parents_lines = parents_lines,
            files_lines = files_lines,
        );
        self.payload.extend_from_slice(commit_payload.as_bytes());
        CommitId(commit_id)
    }

    fn next_id(&mut self) -> usize {
        self.id_count += 1;
        self.id_count
    }
}
