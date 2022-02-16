/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mercurial_types::HgChangesetId;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ErrorKind {
    #[error("Error while uploading data for changesets, hashes: {0:?}")]
    WhileUploadingData(Vec<HgChangesetId>),
}
