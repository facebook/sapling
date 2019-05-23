// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use super::BonsaiHgMappingEntry;
pub use failure_ext::{Error, Fail, Result};

#[derive(Debug, Eq, Fail, PartialEq)]
pub enum ErrorKind {
    #[fail(display = "Connection error")]
    ConnectionError,
    #[fail(display = "Conflicting entries: stored:{:?} current:{:?}", _0, _1)]
    ConflictingEntries(BonsaiHgMappingEntry, BonsaiHgMappingEntry),
    #[fail(
        display = "Conflict detected during insert, but no value was there for: {:?}",
        _0
    )]
    RaceConditionWithDelete(BonsaiHgMappingEntry),
}
