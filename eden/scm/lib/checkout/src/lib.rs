/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Result};
use manifest::{DiffEntry, DiffType, FileType};
use types::{HgId, RepoPathBuf};

/// Contains lists of files to be removed / updated during checkout.
#[allow(dead_code)]
pub struct CheckoutPlan {
    /// Files to be removed.
    remove: Vec<RepoPathBuf>,
    /// Files that needs their content updated.
    update_content: Vec<UpdateContentAction>,
    /// Files that only need X flag updated.
    update_meta: Vec<UpdateMetaAction>,
}

/// Update content and (possibly) metadata on the file
#[allow(dead_code)]
struct UpdateContentAction {
    /// Path to file.
    path: RepoPathBuf,
    /// If content has changed, HgId of new content.
    content_hgid: HgId,
    /// New file type.
    file_type: FileType,
}

/// Only update metadata on the file, do not update content
#[allow(dead_code)]
struct UpdateMetaAction {
    /// Path to file.
    path: RepoPathBuf,
    /// true if need to set executable flag, false if need to remove it.
    set_x_flag: bool,
}

impl CheckoutPlan {
    /// Processes diff into checkout plan.
    /// Left in the diff is a current commit.
    /// Right is a commit to be checked out.
    pub fn from_diff<D: Iterator<Item = Result<DiffEntry>>>(iter: D) -> Result<Self> {
        let mut remove = vec![];
        let mut update_content = vec![];
        let mut update_meta = vec![];
        for item in iter {
            let item: DiffEntry = item?;
            match item.diff_type {
                DiffType::LeftOnly(_) => remove.push(item.path),
                DiffType::RightOnly(meta) => update_content.push(UpdateContentAction {
                    path: item.path,
                    content_hgid: meta.hgid,
                    file_type: meta.file_type,
                }),
                DiffType::Changed(old, new) => {
                    if old.hgid == new.hgid {
                        let set_x_flag = match (old.file_type, new.file_type) {
                            (FileType::Executable, FileType::Regular) => false,
                            (FileType::Regular, FileType::Executable) => true,
                            // todo - address this case
                            // Since this is rare case we are going to handle it by submitting
                            // delete and then create operation to avoid complexity
                            (o, n) => bail!(
                                "Can not update {}: hg id has not changed and file type changed {:?}->{:?}",
                                item.path,
                                o,
                                n
                            ),
                        };
                        update_meta.push(UpdateMetaAction {
                            path: item.path,
                            set_x_flag,
                        });
                    } else {
                        update_content.push(UpdateContentAction {
                            path: item.path,
                            content_hgid: new.hgid,
                            file_type: new.file_type,
                        })
                    }
                }
            };
        }
        Ok(Self {
            remove,
            update_content,
            update_meta,
        })
    }
}
