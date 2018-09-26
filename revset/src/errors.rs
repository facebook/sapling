// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

pub use failure::{Error, Result};

use mercurial_types::{HgChangesetId, HgNodeHash};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "repo error checking for node: {}", _0)] RepoError(HgNodeHash),
    #[fail(display = "could not fetch node generation")] GenerationFetchFailed,
    #[fail(display = "failed to fetch parent nodes")] ParentsFetchFailed,
    #[fail(display = "Bonsai mapping not found for {}", _0)] BonsaiMappingNotFound(HgChangesetId),
}
