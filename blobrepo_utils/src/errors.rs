// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

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
