/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mercurial_types::HgChangesetId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("While visiting changeset {0}")]
    VisitError(HgChangesetId),
    #[error("While verifying changeset {0}")]
    VerificationError(HgChangesetId),
}
