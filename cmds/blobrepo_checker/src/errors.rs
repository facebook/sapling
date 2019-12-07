/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::checks::FileInformation;
use anyhow::Error;
use mercurial_types::HgChangesetId;
use mononoke_types::{hash::Sha256, ChangesetId, ContentId};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Blob hash does not match ChangesetId: looked up {0}, hashed to {1}")]
    BadChangesetHash(ChangesetId, ChangesetId),
    #[error("Changeset {0} is semantically invalid: {1}")]
    InvalidChangeset(ChangesetId, #[source] Error),
    #[error("Changeset {0} has different parents in thrift and database")]
    DbParentsMismatch(ChangesetId),
    #[error("File {0} SHA256 alias does not round trip (SHA256 {1} maps to ContentId {2})")]
    Sha256Mismatch(FileInformation, Sha256, ContentId),
    #[error("File {0} read wrong size {1} in blobstore")]
    BadContentSize(FileInformation, u64),
    #[error("File {0} read wrong ContentId {1} in blobstore")]
    BadContentId(FileInformation, ContentId),
    #[error("Changeset {0} maps to HG {1} maps to wrong Changeset {2}")]
    HgMappingBroken(ChangesetId, HgChangesetId, ChangesetId),
    #[error("Changeset {0} maps to HG {1} which has no matching Bonsai")]
    HgMappingNotPresent(ChangesetId, HgChangesetId),
    #[error("HG {0} has no matching Bonsai")]
    HgDangling(HgChangesetId),
    #[error("HG {0} has different parents to its matching Bonsai")]
    ParentsMismatch(HgChangesetId),
    #[error("Requesting HG {0} fetched changeset {1}")]
    HgChangesetIdMismatch(HgChangesetId, HgChangesetId),
}
