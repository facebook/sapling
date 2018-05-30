// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

pub use failure::{Error, Result};

use mercurial_types::DChangesetId;

use models::ChangesetRow;

#[derive(Debug, Eq, Fail, PartialEq)]
pub enum ErrorKind {
    #[fail(display = "Connection error")] ConnectionError,
    #[fail(display = "Duplicate changeset {} has different parents: {:?} vs {:?}", _0, _1, _2)]
    DuplicateInsertionInconsistency(DChangesetId, Vec<ChangesetRow>, Vec<ChangesetRow>),
    #[fail(display = "Invalid data in database")] InvalidStoredData,
    #[fail(display = "Missing parents")] MissingParents(Vec<DChangesetId>),
}
