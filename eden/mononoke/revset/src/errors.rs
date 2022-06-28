/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

use mercurial_types::HgChangesetId;
use mercurial_types::HgNodeHash;
use mononoke_types::ChangesetId;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("repo error checking for node: {0}")]
    RepoNodeError(HgNodeHash),
    #[error("repo error checking for changeset: {0}")]
    RepoChangesetError(ChangesetId),
    #[error("could not fetch node generation")]
    GenerationFetchFailed,
    #[error("failed to fetch parent nodes")]
    ParentsFetchFailed,
    #[error("Bonsai mapping not found for {0}")]
    BonsaiMappingNotFound(HgChangesetId),
}
