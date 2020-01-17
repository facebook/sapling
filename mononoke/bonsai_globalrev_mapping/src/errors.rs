/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use super::BonsaiGlobalrevMappingEntry;
use thiserror::Error;

#[derive(Debug, Eq, Error, PartialEq)]
pub enum ErrorKind {
    #[error("Conflicting entries: stored:{0:?} current:{1:?}")]
    ConflictingEntries(BonsaiGlobalrevMappingEntry, BonsaiGlobalrevMappingEntry),
    #[error("Conflict detected during insert, but no value was there for: {0:?}")]
    RaceConditionWithDelete(BonsaiGlobalrevMappingEntry),
}
