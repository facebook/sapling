/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use failure::Fail;
pub use failure_ext::{Error, Result};

use mercurial_types::{HgChangesetId, HgNodeHash};
use mononoke_types::ChangesetId;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "repo error checking for node: {}", _0)]
    RepoNodeError(HgNodeHash),
    #[fail(display = "repo error checking for changeset: {}", _0)]
    RepoChangesetError(ChangesetId),
    #[fail(display = "could not fetch node generation")]
    GenerationFetchFailed,
    #[fail(display = "failed to fetch parent nodes")]
    ParentsFetchFailed,
    #[fail(display = "Bonsai mapping not found for {}", _0)]
    BonsaiMappingNotFound(HgChangesetId),
}
