/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

pub use failure_ext::{Error, Result, ResultExt};
use thiserror::Error;

use mercurial_types::HgChangesetId;
use mercurial_types::{HgNodeHash, RepoPath};

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Data corruption for {path}: expected {expected}, actual {actual}!")]
    DataCorruption {
        path: RepoPath,
        expected: HgNodeHash,
        actual: HgNodeHash,
    },
    #[error("Request {request_name} was throttled")]
    RequestThrottled { request_name: String },
    #[error("Bonsai not found for hg changeset: {0:?}")]
    BonsaiNotFoundForHgChangeset(HgChangesetId),
}
