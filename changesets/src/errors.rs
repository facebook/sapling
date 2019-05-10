// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Fail;
pub use failure_ext::{Error, Result};

use mononoke_types::ChangesetId;

#[derive(Debug, Eq, Fail, PartialEq)]
pub enum ErrorKind {
    #[fail(
        display = "Duplicate changeset {} has different parents: {:?} vs {:?}",
        _0, _1, _2
    )]
    DuplicateInsertionInconsistency(ChangesetId, Vec<ChangesetId>, Vec<ChangesetId>),
    #[fail(display = "Missing parents")]
    MissingParents(Vec<ChangesetId>),
}
