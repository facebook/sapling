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
use treestate::treestate::TreeState;

use crate::filesystem::ChangeType;

/// Compute the status of the working copy relative to the current commit.
#[allow(unused_variables)]
pub fn compute_status<M: Matcher + Clone + Send + Sync + 'static>(
    treestate: Arc<Mutex<TreeState>>,
    pending_changes: impl Iterator<Item = ChangeType>,
    matcher: M,
) -> Result<Status> {
    // XXX: Dummy logic to demonstrate the Rust/Python glue.
    let mut modified = vec![];
    let mut removed = vec![];
    for change in pending_changes {
        match change {
            ChangeType::Changed(p) => modified.push(p),
            ChangeType::Deleted(p) => removed.push(p),
        }
    }
    Ok(StatusBuilder::new()
        .modified(modified)
        .removed(removed)
        .build())
}
