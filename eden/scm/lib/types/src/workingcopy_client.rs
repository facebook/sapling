/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::Deserialize;
use serde::Serialize;

use crate::RepoPathBuf;

// edenfs::ScmFileStatus
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub enum FileStatus {
    #[serde(rename = "A")]
    Added,
    #[serde(rename = "M")]
    Modified,
    #[serde(rename = "R")]
    Removed,
    #[serde(rename = "I")]
    Ignored,
    // Ideally there is also an "Error" state (cannot download file).
}

// edenfs::CheckoutMode
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckoutMode {
    Normal,
    Force,
    DryRun,
}

// edenfs::ConflictType
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConflictType {
    /// We failed to update this particular path due to an error.
    Error,
    /// A locally modified file was deleted in the new Tree.
    ModifiedRemoved,
    /// An untracked local file exists in the new Tree.
    UntrackedAdded,
    /// The file was removed locally, but modified in the new Tree.
    RemovedModified,
    /// The file was removed locally, and also removed in the new Tree.
    MissingRemoved,
    /// A locally modified file was modified in the new Tree
    /// This may be contents modifications, or a file type change
    /// (directory to\nfile or vice-versa), or permissions changes.
    ModifiedModified,
    /// A directory was supposed to be removed or replaced with a file,
    /// but it contains untracked files preventing us from updating it.
    DirectoryNotEmpty,
}

// edenfs::CheckoutConflict
#[derive(Clone, Debug, Serialize)]
pub struct CheckoutConflict {
    pub path: RepoPathBuf,
    pub conflict_type: ConflictType,
    pub message: String,
}
