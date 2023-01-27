/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A simple Git copytrace implementation to detect copies by calling git2 library.
//! NOTE: this is a temporary solution to unblock hg-over-git use case.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use types::HgId;

pub struct GitCopyTrace {
    // A git repository instance. Check `GitCopyTrace::open` for more details.
    git_repo: git2::Repository,
}

impl GitCopyTrace {
    /// `open` a GitCopyTrace at `git_dir`.
    pub fn open(git_dir: &Path) -> Result<Self> {
        let git_repo = git2::Repository::open(git_dir)?;
        Ok(GitCopyTrace { git_repo })
    }

    /// Find copies/moves between old and new commits (HgId), the result is
    /// a map of new_path -> old_path
    pub fn find_copies(&self, old_id: HgId, new_id: HgId) -> Result<HashMap<String, String>> {
        let mut map: HashMap<_, _> = HashMap::new();

        let old_tree = self.find_tree(old_id)?;
        let new_tree = self.find_tree(new_id)?;

        let mut diff = self
            .git_repo
            .diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)?;
        diff.find_similar(None)?;

        diff.foreach(
            &mut |file, _progress| {
                match file.status() {
                    git2::Delta::Renamed => {
                        let old_path = diff_file_to_string(file.old_file());
                        let new_path = diff_file_to_string(file.new_file());
                        // skip non-utf8 paths
                        if let (Some(old_path), Some(new_path)) = (old_path, new_path) {
                            map.insert(new_path.to_owned(), old_path.to_owned());
                        }
                    }
                    _ => {}
                }
                true
            },
            None,
            None,
            None,
        )?;

        Ok(map)
    }

    fn find_tree(&self, id: HgId) -> Result<git2::Tree> {
        let oid = git2::Oid::from_bytes(id.as_ref())?;
        let commit = self.git_repo.find_commit(oid)?;
        let tree = commit.tree()?;
        Ok(tree)
    }
}

fn diff_file_to_string(diff_file: git2::DiffFile) -> Option<&str> {
    let path = diff_file
        .path()
        .expect("expect a valid path for the diff file");
    path.to_str()
}
