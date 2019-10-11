/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

pub use failure_ext::{Error, Fail, Result, ResultExt};

use mercurial_types::HgChangesetId;
use mercurial_types::{HgNodeHash, RepoPath};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(
        display = "Data corruption for {}: expected {}, actual {}!",
        _0, _1, _2
    )]
    DataCorruption {
        path: RepoPath,
        expected: HgNodeHash,
        actual: HgNodeHash,
    },
    #[fail(display = "Request {} was throttled", _0)]
    RequestThrottled { request_name: String },
    #[fail(display = "Bonsai not found for hg changeset: {:?}", _0)]
    BonsaiNotFoundForHgChangeset(HgChangesetId),
}
