/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use manifest::FileType;
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
