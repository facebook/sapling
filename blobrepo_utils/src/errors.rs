/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

pub use failure_ext::{Error, Result, ResultExt};
use mercurial_types::HgChangesetId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("While visiting changeset {0}")]
    VisitError(HgChangesetId),
    #[error("While verifying changeset {0}")]
    VerificationError(HgChangesetId),
}
