// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

pub use failure::{Error, Result};

use mercurial_types::DChangesetId;

#[derive(Debug, Eq, Fail, PartialEq)]
pub enum ErrorKind {
    #[fail(display = "Connection error")] ConnectionError,
    #[fail(display = "Changeset already in database")] DuplicateChangeset,
    #[fail(display = "Invalid data in database")] InvalidStoredData,
    #[fail(display = "Missing parents")] MissingParents(Vec<DChangesetId>),
}
