/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

use super::BonsaiHgMappingEntry;

#[derive(Debug, Eq, Error, PartialEq)]
pub enum ErrorKind {
    #[error("Connection error")]
    ConnectionError,
    #[error("Conflicting entries: stored:{0:?} current:{1:?}")]
    ConflictingEntries(BonsaiHgMappingEntry, BonsaiHgMappingEntry),
    #[error("Conflict detected during insert, but no value was there for: {0:?}")]
    RaceConditionWithDelete(BonsaiHgMappingEntry),
}
