/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use manifest::Manifest;
use manifest_tree::ReadTreeManifest;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use parking_lot::RwLock;
use pathmatcher::ExactMatcher;
use pathmatcher::Matcher;
use status::StatusBuilder;
use treestate::filestate::StateFlags;
use treestate::treestate::TreeState;
use types::HgId;
use types::RepoPathBuf;

use crate::filesystem::ChangeType;

struct FakeTreeResolver {
    pub manifest: Arc<RwLock<TreeManifest>>,
}

impl ReadTreeManifest for FakeTreeResolver {
    fn get(&self, _commit_id: &HgId) -> Result<Arc<RwLock<TreeManifest>>> {
        Ok(self.manifest.clone())
    }
}

/// Compute the status of the working copy relative to the current commit.
#[allow(unused_variables)]
pub fn compute_status(
    p1_manifest: &impl Manifest,
    treestate: Arc<Mutex<TreeState>>,
    pending_changes: impl Iterator<Item = Result<ChangeType>>,
    matcher: Arc<dyn Matcher + Send + Sync + 'static>,
) -> Result<StatusBuilder> {
    let mut modified = vec![];
    let mut added = vec![];
    let mut removed = vec![];
    let mut deleted = vec![];
    let mut unknown = vec![];

    // Step 1: get the tree state for each pending change in the working copy.
    // We may have a TreeState that only holds files that are being added/removed
    // (for example, in a repo backed by EdenFS). In this case, we need to make a note
    // of these paths to later query the manifest to determine if they're known or unknown files.

    // Changed files that don't exist in the TreeState. Maps to (is_deleted, in_manifest).
    let mut manifest_files = HashMap::<RepoPathBuf, (bool, bool)>::new();
    for change in pending_changes {
        let (path, is_deleted) = match change {
            Ok(ChangeType::Changed(path)) => (path, false),
            Ok(ChangeType::Deleted(path)) => (path, true),
            Err(e) => return Err(e),
        };

        let mut treestate = treestate.lock();

        let normalized = treestate.normalize(path.as_ref())?;
        let path = RepoPathBuf::from_utf8(normalized.to_vec())?;
        match treestate.get(&normalized)? {
            Some(state) => {
                let exist_parent = state
                    .state
                    .intersects(StateFlags::EXIST_P1 | StateFlags::EXIST_P2);
                let exist_next = state.state.contains(StateFlags::EXIST_NEXT);

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
            None => {
                // Path not found in the TreeState, so we need to query the manifest
                // to determine if this is a known or unknown file.
                manifest_files.insert(path, (is_deleted, false));
            }
        }
    }
    // Handle changed files we didn't find in the TreeState.
    p1_manifest
        .files(ExactMatcher::new(
            manifest_files.keys(),
            true, // case_sensitive = true
        ))
        .filter_map(Result::ok)
        .for_each(|file| {
            if let Some(entry) = manifest_files.get_mut(&file.path) {
                entry.1 = true;
            }
        });
    for (path, (is_deleted, in_manifest)) in manifest_files {
        // `exist_parent = in_manifest`. Also, `exist_parent = in_manifest`:
        // If a file existed in the manifest but didn't EXIST_NEXT,
        // it would be a "removed" file (and thus would definitely be in the TreeState).
        // Similarly, if a file doesn't exist in the manifest but did EXIST_NEXT,
        // it would be an "added" file.
        // This is a subset of the logic above.
        match (is_deleted, in_manifest) {
            (true, true) => deleted.push(path),
            (false, true) => modified.push(path),
            (false, false) => unknown.push(path),
            (true, false) => {} // Deleted, but didn't exist in a parent commit.
        }
    }

    // Step 2: handle files that aren't in pending changes.
    // We can't directly check the filesystem at this layer. Instead, we need to infer:
    // a file that isn't in P1 and isn't in "pending changes" doesn't exist on the filesystem.
    let seen = std::iter::empty()
        .chain(modified.iter())
        .chain(added.iter())
        .chain(removed.iter())
        .chain(deleted.iter())
        .chain(unknown.iter())
        .cloned()
        .collect::<HashSet<RepoPathBuf>>();

    let mut treestate = treestate.lock();

    // A file that's "added" in the tree (doesn't exist in a parent, but exists in the next
    // commit) but isn't in "pending changes" must have been deleted on the filesystem.
    walk_treestate(
        &mut treestate,
        StateFlags::EXIST_NEXT,
        StateFlags::EXIST_P1 | StateFlags::EXIST_P2,
        |path, state| {
            if matcher.matches_file(&path)? && !seen.contains(&path) {
                deleted.push(path);
            }
            Ok(())
        },
    )?;

    // Pending changes shows changes in the working copy with respect to P1.
    // Thus, we need to specially handle files that are in P2.
    walk_treestate(
        &mut treestate,
        StateFlags::EXIST_P2,
        StateFlags::empty(),
        |path, state| {
            // If we saw it in the pending_changes loop earlier, then it's already processed and
            // done.
            if matcher.matches_file(&path)? && !seen.contains(&path) {
                // If it's in P1 but we didn't see it earlier, that means it didn't change with
                // respect to P1. But since it is marked EXIST_P2, that means P2 changed it and
                // therefore we should report it as changed.
                if state.contains(StateFlags::EXIST_P1) {
                    modified.push(path);
                } else {
                    // Since pending changes is with respect to P1, then if it's not in P1
                    // we either saw it in the pending changes loop earlier (in which case
                    // it is in `seen` and was handled), or we didn't see it and therefore
                    // it doesn't exist and is either deleted or removed.
                    if state.contains(StateFlags::EXIST_NEXT) {
                        deleted.push(path);
                    } else {
                        removed.push(path);
                    }
                }
            }
            Ok(())
        },
    )?;

    // Files that will be removed (that is, they exist in either of the parents, but don't
    // exist in the next commit) should be marked as removed, even if they're not in
    // pending changes (e.g. even if the file still exists). Files that are in P2 but
    // not P1 are handled above, so we only need to handle files in P1 here.
    walk_treestate(
        &mut treestate,
        StateFlags::EXIST_P1,
        StateFlags::EXIST_NEXT,
        |path, state| {
            if matcher.matches_file(&path)? && !seen.contains(&path) {
                removed.push(path);
            }
            Ok(())
        },
    )?;

    // Handle "retroactive copies": when a clean file is marked as having been copied
    // from another file. These files should be marked as "modified".
    walk_treestate(
        &mut treestate,
        StateFlags::COPIED,
        StateFlags::empty(),
        |path, state| {
            if matcher.matches_file(&path)? && !seen.contains(&path) {
                modified.push(path);
            }
            Ok(())
        },
    )?;

    Ok(StatusBuilder::new()
        .modified(modified)
        .added(added)
        .removed(removed)
        .deleted(deleted)
        .unknown(unknown))
}

/// Walk the TreeState, calling the callback for files that have all flags in [`state_all`]
/// and none of the flags in [`state_none`].
fn walk_treestate(
    treestate: &mut TreeState,
    state_all: StateFlags,
    state_none: StateFlags,
    mut callback: impl FnMut(RepoPathBuf, StateFlags) -> Result<()>,
) -> Result<()> {
    let file_mask = state_all | state_none;
    treestate.visit(
        &mut |components, state| {
            let path = RepoPathBuf::from_utf8(components.concat())?;
            (callback)(path, state.state)?;
            Ok(treestate::tree::VisitorResult::NotChanged)
        },
        &|_path, dir| match dir.get_aggregated_state() {
            Some(state) => {
                state.union.contains(state_all) && !state.intersection.intersects(state_none)
            }
            None => true,
        },
        &|_path, file| file.state & file_mask == state_all,
    )
}

#[cfg(test)]
mod tests {
    use status::FileStatus;
    use status::Status;
    use tempdir::TempDir;
    use treestate::filestate::FileStateV2;
    use types::RepoPath;
    use types::RepoPathBuf;
    const EXIST_P1: StateFlags = StateFlags::EXIST_P1;
    const EXIST_P2: StateFlags = StateFlags::EXIST_P2;
    const EXIST_NEXT: StateFlags = StateFlags::EXIST_NEXT;
    const COPIED: StateFlags = StateFlags::COPIED;

    use super::*;

    struct DummyManifest {
        files: Vec<RepoPathBuf>,
    }

    #[allow(unused_variables)]
    impl Manifest for DummyManifest {
        fn get(&self, path: &RepoPath) -> Result<Option<manifest::FsNodeMetadata>> {
            unimplemented!()
        }

        fn get_ignore_case(&self, path: &RepoPath) -> Result<Option<manifest::FsNodeMetadata>> {
            unimplemented!("get_ignore_case not implemented for StubCommit")
        }

        fn list(&self, path: &RepoPath) -> Result<manifest::List> {
            unimplemented!()
        }

        fn insert(
            &mut self,
            file_path: RepoPathBuf,
            file_metadata: manifest::FileMetadata,
        ) -> Result<()> {
            unimplemented!()
        }

        fn remove(&mut self, file_path: &RepoPath) -> Result<Option<manifest::FileMetadata>> {
            unimplemented!()
        }

        fn flush(&mut self) -> Result<types::HgId> {
            unimplemented!()
        }

        fn files<'a, M: 'static + Matcher + Sync + Send>(
            &'a self,
            matcher: M,
        ) -> Box<dyn Iterator<Item = Result<manifest::File>> + 'a> {
            Box::new(self.files.iter().cloned().map(|path| {
                Ok(manifest::File {
                    path,
                    meta: manifest::FileMetadata::default(),
                })
            }))
        }

        fn dirs<'a, M: 'static + Matcher + Sync + Send>(
            &'a self,
            matcher: M,
        ) -> Box<dyn Iterator<Item = Result<manifest::Directory>> + 'a> {
            unimplemented!()
        }

        fn diff<'a, M: Matcher>(
            &'a self,
            other: &'a Self,
            matcher: &'a M,
        ) -> Result<Box<dyn Iterator<Item = Result<manifest::DiffEntry>> + 'a>> {
            unimplemented!()
        }

        fn modified_dirs<'a, M: Matcher>(
            &'a self,
            other: &'a Self,
            matcher: &'a M,
        ) -> Result<Box<dyn Iterator<Item = Result<manifest::DirDiffEntry>> + 'a>> {
            unimplemented!()
        }
    }

    /// Compute the status with the given input.
    ///
    /// * `treestate` is a list of (path, state flags).
    /// * `changes` is a list of (path, deleted).
    fn status_helper(treestate: &[(&str, StateFlags)], changes: &[(&str, bool)]) -> Result<Status> {
        // Build the TreeState.
        let dir = TempDir::new("treestate").expect("tempdir");
        let mut state = TreeState::new(dir.path(), true).expect("open").0;
        let mut manifest_files = vec![];
        for (path, flags) in treestate {
            if *flags == (StateFlags::EXIST_P1 | StateFlags::EXIST_NEXT) {
                // Normal file, put it in the manifest instead of the TreeState.
                let path = RepoPathBuf::from_string(path.to_string()).expect("path");
                manifest_files.push(path);
            } else {
                let file_state = FileStateV2 {
                    mode: 0,
                    size: 0,
                    mtime: 0,
                    state: *flags,
                    copied: None,
                };
                state.insert(path, &file_state).expect("insert");
            }
        }
        let treestate = Arc::new(Mutex::new(state));
        let manifest = DummyManifest {
            files: manifest_files,
        };

        // Build the pending changes.
        let changes = changes.iter().map(|&(path, is_deleted)| {
            let path = RepoPathBuf::from_string(path.to_string()).expect("path");
            if is_deleted {
                Ok(ChangeType::Deleted(path))
            } else {
                Ok(ChangeType::Changed(path))
            }
        });

        // Compute the status.
        let matcher = Arc::new(pathmatcher::AlwaysMatcher::new());
        compute_status(&manifest, treestate, changes, matcher).map(StatusBuilder::build)
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

    /// Test status for files that aren't in pending changes.
    #[test]
    fn test_status_no_changes() {
        let treestate = &[
            ("added-then-deleted", EXIST_NEXT),
            ("removed-but-on-filesystem", EXIST_P1),
            ("retroactive-copy", EXIST_P1 | EXIST_NEXT | COPIED),
        ];
        let changes = &[];
        let status = status_helper(treestate, changes).expect("status");
        compare_status(
            status,
            &[
                ("added-then-deleted", Some(FileStatus::Deleted)),
                ("removed-but-on-filesystem", Some(FileStatus::Removed)),
                ("retroactive-copy", Some(FileStatus::Modified)),
            ],
        );
    }

    /// Test status for files relating to a merge.
    #[test]
    fn test_status_merge() {
        let treestate = &[
            ("merged-only-p2", EXIST_P2 | EXIST_NEXT),
            ("merged-in-both", EXIST_P1 | EXIST_P2 | EXIST_NEXT),
            ("merged-and-removed", EXIST_P2),
            ("merged-but-deleted", EXIST_P2 | EXIST_NEXT),
        ];
        let changes = &[("merged-only-p2", false), ("merged-in-both", false)];
        let status = status_helper(treestate, changes).expect("status");
        compare_status(
            status,
            &[
                ("merged-only-p2", Some(FileStatus::Modified)),
                ("merged-in-both", Some(FileStatus::Modified)),
                ("merged-and-removed", Some(FileStatus::Removed)),
                ("merged-but-deleted", Some(FileStatus::Deleted)),
            ],
        );
    }
}
