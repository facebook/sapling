/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use manifest::{DiffEntry, DiffType, FileType};
use types::{HgId, RepoPathBuf};

/// Contains lists of files to be removed / updated during checkout.
#[allow(dead_code)]
pub struct CheckoutPlan {
    /// Files to be removed.
    remove: Vec<RepoPathBuf>,
    /// Files to be updated or created.
    update: Vec<UpdateFileAction>,
}

/// Contains update action on the file.
#[allow(dead_code)]
struct UpdateFileAction {
    /// Path to file.
    path: RepoPathBuf,
    /// If content has changed, HgId of new content.
    content: Option<HgId>,
    /// New file type.
    file_type: FileType,
}

impl CheckoutPlan {
    /// Processes diff into checkout plan.
    /// Left in the diff is a current commit.
    /// Right is a commit to be checked out.
    pub fn from_diff<D: Iterator<Item = Result<DiffEntry>>>(iter: D) -> Result<Self> {
        let mut remove = vec![];
        let mut update = vec![];
        for item in iter {
            let item: DiffEntry = item?;
            match item.diff_type {
                DiffType::LeftOnly(_) => remove.push(item.path),
                DiffType::RightOnly(meta) => update.push(UpdateFileAction {
                    path: item.path,
                    content: Some(meta.hgid),
                    file_type: meta.file_type,
                }),
                DiffType::Changed(old, new) => update.push(UpdateFileAction {
                    path: item.path,
                    content: if_changed(old.hgid, new.hgid),
                    file_type: new.file_type,
                }),
            };
        }
        Ok(Self { remove, update })
    }
}

/// Returns Some(new) if old != new, None otherwise.
fn if_changed<T: PartialEq>(old: T, new: T) -> Option<T> {
    if old == new { None } else { Some(new) }
}
