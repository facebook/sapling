/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::checks::FileInformation;
use failure_ext::{Error, Fail};
use mercurial_types::HgChangesetId;
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
        display = "Changeset {} has different parents in thrift and database",
        _0
    )]
    DbParentsMismatch(ChangesetId),
    #[fail(
        display = "File {} SHA256 alias does not round trip (SHA256 {} maps to ContentId {})",
        _0, _1, _2
    )]
    Sha256Mismatch(FileInformation, Sha256, ContentId),
    #[fail(display = "File {} read wrong size {} in blobstore", _0, _1)]
    BadContentSize(FileInformation, u64),
    #[fail(display = "File {} read wrong ContentId {} in blobstore", _0, _1)]
    BadContentId(FileInformation, ContentId),
    #[fail(
        display = "Changeset {} maps to HG {} maps to wrong Changeset {}",
        _0, _1, _2
    )]
    HgMappingBroken(ChangesetId, HgChangesetId, ChangesetId),
    #[fail(
        display = "Changeset {} maps to HG {} which has no matching Bonsai",
        _0, _1
    )]
    HgMappingNotPresent(ChangesetId, HgChangesetId),
    #[fail(display = "HG {} has no matching Bonsai", _0)]
    HgDangling(HgChangesetId),
    #[fail(display = "HG {} has different parents to its matching Bonsai", _0)]
    ParentsMismatch(HgChangesetId),
    #[fail(display = "Requesting HG {} fetched changeset {}", _0, _1)]
    HgChangesetIdMismatch(HgChangesetId, HgChangesetId),
}
