/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

pub use failure::{Error, Fail};
pub use failure_ext::{Result, ResultExt};
use mercurial_types::HgChangesetId;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "While visiting changeset {}", _0)]
    VisitError(HgChangesetId),
    #[fail(display = "While verifying changeset {}", _0)]
    VerificationError(HgChangesetId),
}
