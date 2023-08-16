/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use anyhow::Result;
use configmodel::Config;
use pathmatcher::DirectoryMatch;
use pathmatcher::DynMatcher;
use pathmatcher::Matcher;
use repolock::RepoLocker;
use treestate::dirstate;
use treestate::filestate::FileStateV2;
use treestate::filestate::StateFlags;
use treestate::treestate::TreeState;
use treestate::ErrorKind;
use types::path::ParseError;
use types::RepoPath;
use types::RepoPathBuf;

/// Walk the TreeState, calling the callback for files that have all flags in [`state_all`]
/// and none of the flags in [`state_none`]. Returns errors parsing invalid paths, if any.
pub fn walk_treestate(
    treestate: &mut TreeState,
    matcher: DynMatcher,
    state_all: StateFlags,
    state_none: StateFlags,
    mut callback: impl FnMut(RepoPathBuf, &FileStateV2) -> Result<()>,
) -> Result<Vec<ParseError>> {
    let file_mask = state_all | state_none;
    let mut path_errors = Vec::new();

    treestate.visit(
        &mut |components, state| {
            match RepoPathBuf::from_utf8(components.concat()) {
                Ok(path) => {
                    if matcher.matches_file(&path)? {
                        (callback)(path, state)?
                    }
                }
                // Ingore but record bad paths. The caller can handle as desired.
                Err(parse_err) => path_errors.push(parse_err),
            };
            Ok(treestate::tree::VisitorResult::NotChanged)
        },
        &|components, dir| {
            let state_matches = match dir.get_aggregated_state() {
                Some(state) => {
                    state.union.contains(state_all) && !state.intersection.intersects(state_none)
                }
                None => true,
            };

            if !state_matches {
                return false;
            }

            if let Ok(dir_path) = RepoPath::from_utf8(&components.concat()) {
                if matches!(
                    matcher.matches_directory(dir_path),
                    Ok(DirectoryMatch::Nothing)
                ) {
                    return false;
                }
            }

            true
        },
        &|_path, file| file.state & file_mask == state_all,
    )?;

    Ok(path_errors)
}

pub(crate) fn dirstate_write_time_override(config: &dyn Config) -> Option<i64> {
    // Respect test fakedirstatewritetime extension.
    if matches!(config.get("extensions", "fakedirstatewritetime"), Some(v) if v != "!") {
        config
            .get("fakedirstatewritetime", "fakenow")
            .map(|time| hgtime::HgTime::parse(time.as_ref()).unwrap().unixtime)
    } else {
        None
    }
}

#[tracing::instrument(skip_all)]
pub(crate) fn maybe_flush_treestate(
    root: &Path,
    ts: &mut TreeState,
    locker: &RepoLocker,
    time_override: Option<i64>,
) -> Result<()> {
    match dirstate::flush(root, ts, locker, time_override) {
        Ok(()) => Ok(()),
        // If the dirstate was changed before we flushed, that's ok. Let the other write win
        // since writes during status are just optimizations.
        Err(e) => match e.downcast_ref::<ErrorKind>() {
            Some(e) if *e == ErrorKind::TreestateOutOfDate => Ok(()),
            _ => Err(e),
        },
    }
}
