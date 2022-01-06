/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

use filestore::FetchKey;
use mercurial_types::HgChangesetId;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Bonsai not found for hg changeset: {0:?}")]
    BonsaiNotFoundForHgChangeset(HgChangesetId),
    #[error("missing content {0:?}")]
    MissingContent(FetchKey),
}
