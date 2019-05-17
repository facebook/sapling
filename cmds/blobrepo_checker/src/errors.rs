// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::checks::FileInformation;
use failure_ext::{Error, Fail};
use mononoke_types::{hash::Sha256, ChangesetId, ContentId};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(
        display = "Blob hash does not match ChangesetId: looked up {}, hashed to {}",
        _0, _1
    )]
    BadChangesetHash(ChangesetId, ChangesetId),
    #[fail(display = "Changeset {} is semantically invalid: {}", _0, _1)]
    InvalidChangeset(ChangesetId, #[fail(cause)] Error),
    #[fail(
        display = "File {} SHA256 alias does not round trip (SHA256 {} maps to ContentId {})",
        _0, _1, _2
    )]
    Sha256Mismatch(FileInformation, Sha256, ContentId),
    #[fail(display = "File {} read wrong size {} in blobstore", _0, _1)]
    BadContentSize(FileInformation, usize),
    #[fail(display = "File {} read wrong ContentId {} in blobstore", _0, _1)]
    BadContentId(FileInformation, ContentId),
}
