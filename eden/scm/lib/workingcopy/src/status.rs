/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use parking_lot::Mutex;
use pathmatcher::Matcher;
use status::{Status, StatusBuilder};
use treestate::filestate::StateFlags;
use treestate::treestate::TreeState;

use crate::filesystem::ChangeType;

/// Compute the status of the working copy relative to the current commit.
#[allow(unused_variables)]
pub fn compute_status<M: Matcher + Clone + Send + Sync + 'static>(
    treestate: Arc<Mutex<TreeState>>,
    pending_changes: impl Iterator<Item = ChangeType>,
    matcher: M,
) -> Result<Status> {
    let mut modified = vec![];
    let mut added = vec![];
    let mut removed = vec![];
    let mut deleted = vec![];
    let mut unknown = vec![];

    // Step 1: get the tree state for each pending change in the working copy.
    let mut treestate = treestate.lock();
    for change in pending_changes {
        let (path, is_deleted) = match change {
            ChangeType::Changed(path) => (path, false),
            ChangeType::Deleted(path) => (path, true),
        };

        let (exist_parent, exist_next) = match treestate.get(&path)? {
            Some(state) => {
                let parent = state
                    .state
                    .intersects(StateFlags::EXIST_P1 | StateFlags::EXIST_P2);
                let next = state.state.contains(StateFlags::EXIST_NEXT);
                (parent, next)
            }
            None => (false, false),
        };

        match (is_deleted, exist_parent, exist_next) {
            (_, true, false) => removed.push(path),
            (true, true, true) => deleted.push(path),
            (false, true, true) => modified.push(path),
            (false, false, true) => added.push(path),
            (false, false, false) => unknown.push(path),
            _ => {
                // The remaining case is (T, F, _).
                // If the file is deleted, but didn't exist in a parent commit,
                // it didn't change.
            }
        }
    }

    Ok(StatusBuilder::new()
        .modified(modified)
        .added(added)
        .removed(removed)
        .deleted(deleted)
        .unknown(unknown)
        .build())
}

#[cfg(test)]
mod tests {
    use status::FileStatus;
    use tempdir::TempDir;
    use treestate::filestate::FileStateV2;
    use types::RepoPath;
    use types::RepoPathBuf;
    const EXIST_P1: StateFlags = StateFlags::EXIST_P1;
    const EXIST_NEXT: StateFlags = StateFlags::EXIST_NEXT;

    use super::*;

    /// Compute the status with the given input.
    ///
    /// * `treestate` is a list of (path, state flags).
    /// * `changes` is a list of (path, deleted).
    fn status_helper(treestate: &[(&str, StateFlags)], changes: &[(&str, bool)]) -> Result<Status> {
        // Build the TreeState.
        let dir = TempDir::new("treestate").expect("tempdir");
        let mut state = TreeState::open(dir.path().join("1"), None).expect("open");
        for (path, flags) in treestate {
            let file_state = FileStateV2 {
                mode: 0,
                size: 0,
                mtime: 0,
                state: *flags,
                copied: None,
            };
            state.insert(path, &file_state).expect("insert");
        }
        let treestate = Arc::new(Mutex::new(state));

        // Build the pending changes.
        let changes = changes.iter().map(|&(path, is_deleted)| {
            let path = RepoPathBuf::from_string(path.to_string()).expect("path");
            if is_deleted {
                ChangeType::Deleted(path)
            } else {
                ChangeType::Changed(path)
            }
        });

        // Compute the status.
        let matcher = pathmatcher::AlwaysMatcher::new();
        compute_status(treestate, changes, matcher)
    }

    /// Compare the [`Status`] with the expected status for each given file.
    fn compare_status(status: Status, expected_list: &[(&str, Option<FileStatus>)]) {
        for (path, expected) in expected_list {
            let actual = status.status(RepoPath::from_str(path).expect("path"));
            assert_eq!(&actual, expected, "status for '{}'", path);
        }
    }

    /// Test status for files in pending changes.
    #[test]
    fn test_status_pending_changes() {
        let treestate = &[
            ("normal-file", EXIST_P1 | EXIST_NEXT),
            ("modified-file", EXIST_P1 | EXIST_NEXT),
            ("added-file", EXIST_NEXT),
            ("removed-file", EXIST_P1),
            ("deleted-file", EXIST_P1 | EXIST_NEXT),
        ];
        let changes = &[
            ("modified-file", false),
            ("added-file", false),
            ("removed-file", true),
            ("deleted-file", true),
            ("unknown-file", false),
        ];
        let status = status_helper(treestate, changes).expect("status");
        compare_status(
            status,
            &[
                ("normal-file", None),
                ("modified-file", Some(FileStatus::Modified)),
                ("added-file", Some(FileStatus::Added)),
                ("removed-file", Some(FileStatus::Removed)),
                ("deleted-file", Some(FileStatus::Deleted)),
                ("unknown-file", Some(FileStatus::Unknown)),
            ],
        );
    }
}
