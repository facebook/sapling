/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod caching;
mod sql;
mod types;

pub use crate::caching::CachingSyncedCommitMapping;
pub use crate::sql::add_many_in_txn;
pub use crate::sql::add_many_large_repo_commit_versions_in_txn;
pub use crate::sql::SqlSyncedCommitMapping;
pub use crate::sql::SqlSyncedCommitMappingBuilder;
pub use crate::types::ArcSyncedCommitMapping;
pub use crate::types::EquivalentWorkingCopyEntry;
pub use crate::types::ErrorKind;
pub use crate::types::FetchedMappingEntry;
pub use crate::types::SyncedCommitMapping;
pub use crate::types::SyncedCommitMappingArc;
pub use crate::types::SyncedCommitMappingEntry;
pub use crate::types::SyncedCommitMappingRef;
pub use crate::types::SyncedCommitSourceRepo;
pub use crate::types::WorkingCopyEquivalence;
