/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use treestate::filestate::FileStateV2;
use treestate::filestate::StateFlags;
use treestate::treestate::TreeState;
use types::path::ParseError;
use types::RepoPathBuf;

/// Walk the TreeState, calling the callback for files that have all flags in [`state_all`]
/// and none of the flags in [`state_none`]. Returns errors parsing invalid paths, if any.
pub fn walk_treestate(
    treestate: &mut TreeState,
    state_all: StateFlags,
    state_none: StateFlags,
    mut callback: impl FnMut(RepoPathBuf, &FileStateV2) -> Result<()>,
) -> Result<Vec<ParseError>> {
    let file_mask = state_all | state_none;
    let mut path_errors = Vec::new();

    treestate.visit(
        &mut |components, state| {
            match RepoPathBuf::from_utf8(components.concat()) {
                Ok(path) => (callback)(path, state)?,
                // Ingore but record bad paths. The caller can handle as desired.
                Err(parse_err) => path_errors.push(parse_err),
            };
            Ok(treestate::tree::VisitorResult::NotChanged)
        },
        &|_path, dir| match dir.get_aggregated_state() {
            Some(state) => {
                state.union.contains(state_all) && !state.intersection.intersects(state_none)
            }
            None => true,
        },
        &|_path, file| file.state & file_mask == state_all,
    )?;

    Ok(path_errors)
}
