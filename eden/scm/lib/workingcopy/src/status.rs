/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use manifest::Manifest;
use parking_lot::Mutex;
use pathmatcher::DifferenceMatcher;
use pathmatcher::DynMatcher;
use pathmatcher::ExactMatcher;
use status::StatusBuilder;
use tracing::trace;
use treestate::filestate::StateFlags;
use treestate::treestate::TreeState;
use types::RepoPathBuf;

use crate::filesystem::PendingChange;
use crate::util::walk_treestate;
use crate::walker::WalkError;

/// Compute the status of the working copy relative to the current commit.
#[allow(unused_variables)]
#[tracing::instrument(skip_all)]
pub fn compute_status(
    p1_manifest: &impl Manifest,
    treestate: Arc<Mutex<TreeState>>,
    pending_changes: impl Iterator<Item = Result<PendingChange>>,
    matcher: DynMatcher,
) -> Result<StatusBuilder> {
    let mut modified = vec![];
    let mut added = vec![];
    let mut removed = vec![];
    let mut deleted = vec![];
    let mut unknown = vec![];
    let mut ignored = vec![];
    let mut invalid_path = vec![];
    let mut invalid_type = vec![];

    // Step 1: get the tree state for each pending change in the working copy.
    // We may have a TreeState that only holds files that are being added/removed
    // (for example, in a repo backed by EdenFS). In this case, we need to make a note
    // of these paths to later query the manifest to determine if they're known or unknown files.

    // Changed files that don't exist in the TreeState. Maps to (is_deleted, in_manifest).
    let mut manifest_files = HashMap::<RepoPathBuf, (bool, bool)>::new();
    for change in pending_changes {
        let (path, is_deleted) = match change {
            Ok(PendingChange::Changed(path)) => (path, false),
            Ok(PendingChange::Deleted(path)) => (path, true),
            Ok(PendingChange::Ignored(path)) => {
                ignored.push(path);
                continue;
            }
            Err(e) => {
                let e = match e.downcast::<types::path::ParseError>() {
                    Ok(parse_err) => {
                        invalid_path.push(parse_err.into_path_bytes());
                        continue;
                    }
                    Err(e) => e,
                };

                let e = match e.downcast::<WalkError>() {
                    Ok(walk_err) => {
                        match walk_err {
                            WalkError::InvalidFileType(path) => {
                                invalid_type.push(path);
                                continue;
                            }
                            WalkError::RepoPathError(_, err) => {
                                invalid_path.push(err.into_path_bytes());
                                continue;
                            }
                            WalkError::FsUtf8Error(path) => {
                                invalid_path.push(path.into_bytes());
                                continue;
                            }
                            _ => {}
                        }

                        walk_err.into()
                    }
                    Err(e) => e,
                };

                return Err(e);
            }
        };

        let mut treestate = treestate.lock();

        // Don't use normalized_get since we need to support statuses like:
        //   $ sl status
        //   R foo
        //   ? FOO
        match treestate.get(&path)? {
            Some(state) => {
                let exist_parent = state
                    .state
                    .intersects(StateFlags::EXIST_P1 | StateFlags::EXIST_P2);
                let exist_next = state.state.contains(StateFlags::EXIST_NEXT);
                let copied = state.state.contains(StateFlags::COPIED);

                trace!(%path, is_deleted, exist_parent, exist_next, copied);

                match (is_deleted, exist_parent, exist_next, copied) {
                    (_, true, false, _) => removed.push(path),
                    (true, true, true, _) => deleted.push(path),
                    (false, true, true, _) => modified.push(path),
                    (false, false, true, _) => added.push(path),
                    // This happens on EdenFS when a modified file is
                    // renamed over another existing file.
                    (false, false, false, true) => modified.push(path),
                    (false, false, false, false) => unknown.push(path),
                    (true, false, true, _) => deleted.push(path),
                    (true, false, false, _) => {}
                }
            }
            None => {
                trace!(%path, is_deleted, "not in dirstate");

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
        trace!(%path, is_deleted, in_manifest, "manifest file");

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
        .chain(ignored.iter());

    // Augment matcher to skip "seen" files since they have already been handled above.
    let matcher = Arc::new(DifferenceMatcher::new(
        matcher,
        ExactMatcher::new(seen, true),
    ));

    let mut treestate = treestate.lock();

    // A file that's "added" in the tree (doesn't exist in a parent, but exists in the next
    // commit) but isn't in "pending changes" must have been deleted on the filesystem.
    walk_treestate(
        &mut treestate,
        matcher.clone(),
        StateFlags::EXIST_NEXT,
        StateFlags::empty(),
        StateFlags::EXIST_P1 | StateFlags::EXIST_P2,
        |path, state| {
            trace!(%path, "deleted (added file not in pending changes)");
            deleted.push(path);
            Ok(())
        },
    )?;

    // Pending changes shows changes in the working copy with respect to P1.
    // Thus, we need to specially handle files that are in P2.
    walk_treestate(
        &mut treestate,
        matcher.clone(),
        StateFlags::EXIST_P2,
        StateFlags::empty(),
        StateFlags::empty(),
        |path, state| {
            // If it's in P1 but we didn't see it earlier, that means it didn't change with
            // respect to P1. But since it is marked EXIST_P2, that means P2 changed it and
            // therefore we should report it as changed.
            if state.state.contains(StateFlags::EXIST_P1) {
                trace!(%path, "modified (infer p2 modified)");
                modified.push(path);
            } else {
                // Since pending changes is with respect to P1, then if it's not in P1
                // we either saw it in the pending changes loop earlier (in which case
                // it is in `seen` and was handled), or we didn't see it and therefore
                // it doesn't exist and is either deleted or removed.
                if state.state.contains(StateFlags::EXIST_NEXT) {
                    trace!(%path, "deleted (in p2, in next, not in pending changes)");
                    deleted.push(path);
                } else {
                    trace!(%path, "removed (in p2, not in next, not in pending changes)");
                    removed.push(path);
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
        matcher.clone(),
        StateFlags::EXIST_P1,
        StateFlags::empty(),
        StateFlags::EXIST_NEXT,
        |path, state| {
            trace!(%path, "removed (in p1, not in next, not in pending changes)");
            removed.push(path);
            Ok(())
        },
    )?;

    // Handle "retroactive copies": when a clean file is marked as having been copied
    // from another file. These files should be marked as "modified".
    walk_treestate(
        &mut treestate,
        matcher.clone(),
        StateFlags::COPIED | StateFlags::EXIST_NEXT,
        StateFlags::empty(),
        StateFlags::empty(),
        |path, state| {
            trace!(%path, "modified (marked copy, not in pending changes)");
            modified.push(path);
            Ok(())
        },
    )?;

    Ok(StatusBuilder::new()
        // Ignored added files can show up as both ignored and added.
        // Work around by inserting ignored first so added files
        // "win". The better fix is to augment the ignore matcher to
        // not include added files, which almost works except for
        // EdenFS, which doesn't report added ignored files as added.
        .ignored(ignored)
        .modified(modified)
        .added(added)
        .removed(removed)
        .deleted(deleted)
        .unknown(unknown)
        .invalid_path(invalid_path)
        .invalid_type(invalid_type))
}

#[cfg(test)]
mod tests {
    use pathmatcher::Matcher;
    use status::FileStatus;
    use status::Status;
    use tempfile::TempDir;
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
        let dir = TempDir::with_prefix("treestate.").expect("tempdir");
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
                Ok(PendingChange::Deleted(path))
            } else {
                Ok(PendingChange::Changed(path))
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
