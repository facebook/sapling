/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use super::BonsaiGlobalrevMappingEntry;
pub use failure_ext::{Error, Fail, Result};

#[derive(Debug, Eq, Fail, PartialEq)]
pub enum ErrorKind {
    #[fail(display = "Conflicting entries: stored:{:?} current:{:?}", _0, _1)]
    ConflictingEntries(BonsaiGlobalrevMappingEntry, BonsaiGlobalrevMappingEntry),
    #[fail(
        display = "Conflict detected during insert, but no value was there for: {:?}",
        _0
    )]
    RaceConditionWithDelete(BonsaiGlobalrevMappingEntry),
}
