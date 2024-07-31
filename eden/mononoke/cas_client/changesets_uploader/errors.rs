/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgFileNodeId;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use mononoke_types::MPathElement;

#[derive(thiserror::Error, Debug)]
pub enum CasChangesetUploaderErrorKind {
    #[error("The following changeset is unexpectedly missing: {0}")]
    InvalidChangeset(ChangesetId),
    #[error("Diff changeset's manifest with its parents failed: {0}")]
    DiffChangesetFailed(Error),
    #[error("Upload failed for filenode id: {0}, filename {1}, with error: {2}")]
    FileUploadFailed(HgFileNodeId, MPathElement, Error),
    #[error("Upload failed for filenode id: {0}, path {1}, with error: {2}")]
    FileUploadFailedWithFullPath(HgFileNodeId, MPath, Error),
    #[error("Upload failed for augmented manifest id: {0} with error: {1}")]
    TreeUploadFailed(HgAugmentedManifestId, Error),
    #[error("Path not found: {0}")]
    PathNotFound(MPath),
    #[error(transparent)]
    Error(#[from] Error),
}
