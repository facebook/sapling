/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use pathmatcher::AlwaysMatcher;
use pathmatcher::DynMatcher;
use pathmatcher::NeverMatcher;
use treestate::filestate::FileStateV2;
use treestate::filestate::StateFlags;
use treestate::treestate::TreeState;
use types::RepoPathBuf;

use super::watchmanfs::detect_changes;
use crate::filechangedetector::FileChangeDetectorTrait;
use crate::filechangedetector::ResolvedFileChangeResult;
use crate::filesystem::PendingChange;
use crate::metadata;

const NEED_CHECK: StateFlags = StateFlags::NEED_CHECK;
const EXIST_P1: StateFlags = StateFlags::EXIST_P1;
const EXIST_NEXT: StateFlags = StateFlags::EXIST_NEXT;

#[derive(Default)]
struct TestFileChangeDetector {
    changed_files: Vec<RepoPathBuf>,
    deleted_files: Vec<RepoPathBuf>,

    results: Vec<Result<ResolvedFileChangeResult>>,
}

impl FileChangeDetectorTrait for TestFileChangeDetector {
    fn submit(&mut self, file: metadata::File) {
        if self.changed_files.contains(&file.path) {
            self.results
                .push(Ok(ResolvedFileChangeResult::Yes(PendingChange::Changed(
                    file.path,
                ))));
        } else if self.deleted_files.contains(&file.path) {
            self.results
                .push(Ok(ResolvedFileChangeResult::Yes(PendingChange::Deleted(
                    file.path,
                ))));
        } else {
            self.results
                .push(Ok(ResolvedFileChangeResult::No((file.path, None))));
        }
    }
}

impl IntoIterator for TestFileChangeDetector {
    type Item = Result<ResolvedFileChangeResult>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.results.into_iter()
    }
}

#[derive(PartialEq, Debug, Copy, Clone)]
enum Change {
    Changed,
    Deleted,
}

use Change::*;

#[derive(Debug)]
enum MatcherMode {
    Always,
    Never,
}

use MatcherMode::*;

#[derive(Debug)]
struct TestCase {
    // initial treestate state for this file
    state_before: Option<StateFlags>,
    // whether watchman reported this file to us
    wm_changed: bool,
    // watchman is_fresh_instance (i.e. watchman restarted)
    wm_fresh_instance: bool,
    // whether the file appears to have changed/deleted when we check disk
    disk_changed: Option<Change>,
    // expected treestate after the dust settles (None means should be deleted)
    state_after: Option<StateFlags>,
    // what kind of pending change should be produced, if any
    pending_change: Option<Change>,
    // matcher to limit output of status
    matcher: MatcherMode,
}

fn check(mut tc: TestCase) -> Result<()> {
    let dir = tempfile::tempdir()?;

    let mut ts = TreeState::new(dir.path(), false)?.0;
    let path = RepoPathBuf::from_string("some_path".to_string())?;

    if let Some(state_before) = tc.state_before.clone() {
        ts.insert(
            &path,
            &FileStateV2 {
                mode: 0,
                size: 0,
                mtime: 0,
                copied: None,
                state: state_before,
            },
        )?;
    }

    let mut stub_detector = TestFileChangeDetector::default();
    if let Some(disk_changed) = tc.disk_changed.take() {
        match disk_changed {
            Change::Changed => stub_detector.changed_files.push(path.clone()),
            Change::Deleted => stub_detector.deleted_files.push(path.clone()),
        }
    }

    let mut wm_changes = Vec::new();
    if tc.wm_changed {
        wm_changes.push(path.clone());
    }

    let matcher: DynMatcher = match tc.matcher {
        Always => Arc::new(AlwaysMatcher::new()),
        Never => Arc::new(NeverMatcher::new()),
    };

    //
    // This is the code under test.
    //
    let mut changes = detect_changes(
        matcher,
        Arc::new(NeverMatcher::new()),
        false,
        false,
        stub_detector,
        &mut ts,
        wm_changes
            .into_iter()
            .map(|p| metadata::File {
                path: p,
                fs_meta: None,
                ts_state: None,
            })
            .collect(),
        tc.wm_fresh_instance,
        true,
    )?;

    changes.update_treestate(&mut ts)?;

    let state = ts.get(&path)?;

    if let Some(state_after) = tc.state_after.clone() {
        assert!(
            state.is_some(),
            "expected file to be in treestate {:?}",
            &tc
        );
        assert_eq!(state.unwrap().state, state_after, "{:?}", &tc);
    } else {
        assert!(
            state.is_none(),
            "expected file to not be in treestate {:?}",
            &tc
        );
    }

    let mut pending_changes: Vec<_> = changes.into_iter().collect();
    if let Some(want_change) = tc.pending_change.clone() {
        assert!(pending_changes.len() == 1, "{:?}", &tc);
        if !pending_changes.is_empty() {
            match pending_changes.pop().unwrap().unwrap() {
                PendingChange::Changed(got_path) => {
                    assert_eq!(path, got_path);
                    assert_eq!(want_change, Change::Changed);
                }
                PendingChange::Deleted(got_path) => {
                    assert_eq!(path, got_path);
                    assert_eq!(want_change, Change::Deleted);
                }
                PendingChange::Ignored(got_path) => {
                    panic!("got ignored file {:?}", got_path);
                }
            }
        }
    } else {
        assert_eq!(pending_changes.len(), 0, "{:?}", &tc);
    }

    Ok(())
}

fn product(flags: &[StateFlags]) -> Vec<StateFlags> {
    let len = 1 << flags.len();
    let mut result = Vec::with_capacity(len);
    for bits in 0..len {
        let mut flag = StateFlags::empty();
        for (i, &f) in flags.iter().enumerate() {
            if (bits & (1 << i)) != 0 {
                flag |= f;
            }
        }
        result.push(flag);
    }
    result
}

#[test]
fn test_detect_changes() -> Result<()> {
    const NEED_CHECK: StateFlags = StateFlags::NEED_CHECK;
    const EXIST_P1: StateFlags = StateFlags::EXIST_P1;
    const EXIST_NEXT: StateFlags = StateFlags::EXIST_NEXT;

    let all_states = product(&[NEED_CHECK, EXIST_P1, EXIST_NEXT]);
    for state_before in all_states {
        for wm_changed in [true, false] {
            for disk_changed in [Some(Changed), Some(Deleted), None] {
                // Expected state_after and pending_changes
                let state_after = {
                    let mut s = state_before;
                    match (wm_changed, &disk_changed, s.contains(NEED_CHECK)) {
                        (true, None, _) | (_, None, true) => {
                            s -= NEED_CHECK;
                        }
                        (true, Some(_), _) => {
                            s |= NEED_CHECK;
                        }
                        _ => {}
                    }
                    s
                };
                let pending_change = if wm_changed || state_before.contains(NEED_CHECK) {
                    disk_changed
                } else {
                    None
                };
                // Normalize states to Option form.
                let state_before = if state_before.is_empty() {
                    None
                } else {
                    Some(state_before)
                };
                let state_after = if state_after.is_empty() {
                    None
                } else {
                    Some(state_after)
                };
                check(TestCase {
                    state_before,
                    wm_changed,
                    disk_changed,
                    state_after,
                    pending_change,
                    wm_fresh_instance: false,
                    matcher: Always,
                })?;
            }
        }
    }

    Ok(())
}

#[test]
fn test_detect_changes_fresh_instance() -> Result<()> {
    check(TestCase {
        state_before: Some(EXIST_P1 | EXIST_NEXT),
        wm_changed: false,
        wm_fresh_instance: true,
        disk_changed: Some(Deleted),
        state_after: Some(EXIST_P1 | EXIST_NEXT | NEED_CHECK),
        pending_change: Some(Deleted),
        matcher: Always,
    })?;

    check(TestCase {
        state_before: Some(NEED_CHECK),
        wm_changed: false,
        wm_fresh_instance: true,
        disk_changed: Some(Deleted),
        state_after: None,
        pending_change: Some(Deleted),
        matcher: Always,
    })?;

    Ok(())
}

#[test]
fn test_never_matcher() -> Result<()> {
    // Make sure a non-matching matcher doesn't mess up correctness of
    // what is recorded in treestate.

    check(TestCase {
        state_before: Some(EXIST_P1 | EXIST_NEXT),
        wm_changed: true,
        wm_fresh_instance: false,
        disk_changed: Some(Changed),
        state_after: Some(EXIST_P1 | EXIST_NEXT | NEED_CHECK),
        // Perhaps we don't need to produce the pending change, but it gets
        // filtered later by the matcher.
        pending_change: Some(Changed),
        matcher: Never,
    })?;
    check(TestCase {
        state_before: Some(EXIST_P1 | EXIST_NEXT | NEED_CHECK),
        wm_changed: false,
        wm_fresh_instance: false,
        disk_changed: Some(Changed),
        state_after: Some(EXIST_P1 | EXIST_NEXT | NEED_CHECK),
        pending_change: None,
        matcher: Never,
    })?;
    check(TestCase {
        state_before: None,
        wm_changed: true,
        wm_fresh_instance: false,
        disk_changed: Some(Changed),
        state_after: Some(NEED_CHECK),
        pending_change: Some(Changed),
        matcher: Never,
    })?;
    check(TestCase {
        state_before: None,
        wm_changed: true,
        wm_fresh_instance: true,
        disk_changed: Some(Changed),
        state_after: Some(NEED_CHECK),
        pending_change: Some(Changed),
        matcher: Never,
    })?;

    Ok(())
}
