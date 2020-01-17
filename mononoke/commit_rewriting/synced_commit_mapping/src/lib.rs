/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use std::sync::Arc;

use sql::{Connection, Transaction};
pub use sql_ext::SqlConstructors;

use anyhow::Error;
use cloned::cloned;
use context::CoreContext;
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::{ChangesetId, RepositoryId};
use sql::queries;
use stats::prelude::*;
use thiserror::Error;

#[derive(Debug, Eq, Error, PartialEq)]
pub enum ErrorKind {
    #[error("tried to insert inconsistent small bcs id {expected:?}, while db has {actual:?}")]
    InconsistentWorkingCopyEntry {
        expected: Option<ChangesetId>,
        actual: Option<ChangesetId>,
    },
}

// TODO(simonfar): Once we've proven the concept, we want to cache these
define_stats! {
    prefix = "mononoke.synced_commit_mapping";
    gets: timeseries(Rate, Sum),
    gets_master: timeseries(Rate, Sum),
    adds: timeseries(Rate, Sum),
    add_no_commit: timeseries(Rate, Sum),
    add_bulks: timeseries(Rate, Sum),
    insert_working_copy_eqivalence: timeseries(Rate, Sum),
    get_equivalent_working_copy: timeseries(Rate, Sum),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SyncedCommitMappingEntry {
    pub large_repo_id: RepositoryId,
    pub large_bcs_id: ChangesetId,
    pub small_repo_id: RepositoryId,
    pub small_bcs_id: ChangesetId,
}

impl SyncedCommitMappingEntry {
    pub fn new(
        large_repo_id: RepositoryId,
        large_bcs_id: ChangesetId,
        small_repo_id: RepositoryId,
        small_bcs_id: ChangesetId,
    ) -> Self {
        Self {
            large_repo_id,
            large_bcs_id,
            small_repo_id,
            small_bcs_id,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EquivalentWorkingCopyEntry {
    pub large_repo_id: RepositoryId,
    pub large_bcs_id: ChangesetId,
    pub small_repo_id: RepositoryId,
    pub small_bcs_id: Option<ChangesetId>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WorkingCopyEquivalence {
    /// There's no matching working copy. It can happen if a pre-big-merge commit from one small
    /// repo is mapped into another small repo
    NoWorkingCopy,
    /// ChangesetId of matching working copy.
    WorkingCopy(ChangesetId),
}

pub trait SyncedCommitMapping: Send + Sync {
    /// Given the full large, small mapping, store it in the DB.
    /// Future resolves to true if the mapping was saved, false otherwise
    fn add(&self, ctx: CoreContext, entry: SyncedCommitMappingEntry) -> BoxFuture<bool, Error>;

    /// Adds a new entry, but doesn't commit a transaction, allowing using transaction
    /// to make another update with it.
    fn add_no_commit(
        &self,
        ctx: CoreContext,
        entry: SyncedCommitMappingEntry,
    ) -> BoxFuture<Transaction, Error>;

    /// Bulk insert a set of large, small mappings
    /// This is meant for blobimport and similar
    fn add_bulk(
        &self,
        ctx: CoreContext,
        entries: Vec<SyncedCommitMappingEntry>,
    ) -> BoxFuture<u64, Error>;

    /// Find the mapping entry for a given source commit and target repo
    fn get(
        &self,
        ctx: CoreContext,
        source_repo_id: RepositoryId,
        bcs_id: ChangesetId,
        target_repo_id: RepositoryId,
    ) -> BoxFuture<Option<ChangesetId>, Error>;

    /// Inserts equivalent working copy of a large bcs id. It's similar to mapping entry,
    /// however there are a few differences:
    /// 1) For (large repo, small repo) pair, many large commits can map to the same small commit
    /// 2) Small commit can be null
    ///
    /// If there's a mapping between small and large commits, then equivalent working copy is
    /// the same as the same as the mapping.
    fn insert_equivalent_working_copy(
        &self,
        ctx: CoreContext,
        entry: EquivalentWorkingCopyEntry,
    ) -> BoxFuture<bool, Error>;

    /// Finds equivalent working copy
    fn get_equivalent_working_copy(
        &self,
        ctx: CoreContext,
        source_repo_id: RepositoryId,
        source_bcs_id: ChangesetId,
        target_repo_id: RepositoryId,
    ) -> BoxFuture<Option<WorkingCopyEquivalence>, Error>;
}

impl SyncedCommitMapping for Arc<dyn SyncedCommitMapping> {
    fn add(&self, ctx: CoreContext, entry: SyncedCommitMappingEntry) -> BoxFuture<bool, Error> {
        (**self).add(ctx, entry)
    }

    fn add_no_commit(
        &self,
        ctx: CoreContext,
        entry: SyncedCommitMappingEntry,
    ) -> BoxFuture<Transaction, Error> {
        (**self).add_no_commit(ctx, entry)
    }

    fn add_bulk(
        &self,
        ctx: CoreContext,
        entries: Vec<SyncedCommitMappingEntry>,
    ) -> BoxFuture<u64, Error> {
        (**self).add_bulk(ctx, entries)
    }

    fn get(
        &self,
        ctx: CoreContext,
        source_repo_id: RepositoryId,
        bcs_id: ChangesetId,
        target_repo_id: RepositoryId,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        (**self).get(ctx, source_repo_id, bcs_id, target_repo_id)
    }

    fn insert_equivalent_working_copy(
        &self,
        ctx: CoreContext,
        entry: EquivalentWorkingCopyEntry,
    ) -> BoxFuture<bool, Error> {
        (**self).insert_equivalent_working_copy(ctx, entry)
    }

    fn get_equivalent_working_copy(
        &self,
        ctx: CoreContext,
        source_repo_id: RepositoryId,
        source_bcs_id: ChangesetId,
        target_repo_id: RepositoryId,
    ) -> BoxFuture<Option<WorkingCopyEquivalence>, Error> {
        (**self).get_equivalent_working_copy(ctx, source_repo_id, source_bcs_id, target_repo_id)
    }
}

#[derive(Clone)]
pub struct SqlSyncedCommitMapping {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
}

queries! {
    write InsertMapping(values: (
        large_repo_id: RepositoryId,
        large_bcs_id: ChangesetId,
        small_repo_id: RepositoryId,
        small_bcs_id: ChangesetId,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO synced_commit_mapping (large_repo_id, large_bcs_id, small_repo_id, small_bcs_id) VALUES {values}"
    }

    read SelectMapping(
        source_repo_id: RepositoryId,
        bcs_id: ChangesetId,
        target_repo_id: RepositoryId,
    ) -> (RepositoryId, ChangesetId, RepositoryId, ChangesetId) {
        "SELECT large_repo_id, large_bcs_id, small_repo_id, small_bcs_id
         FROM synced_commit_mapping
         WHERE (large_repo_id = {source_repo_id} AND large_bcs_id = {bcs_id} AND small_repo_id = {target_repo_id}) OR
         (small_repo_id = {source_repo_id} AND small_bcs_id = {bcs_id} AND large_repo_id = {target_repo_id})"
    }

    write InsertWorkingCopyEquivalence(values: (
        large_repo_id: RepositoryId,
        large_bcs_id: ChangesetId,
        small_repo_id: RepositoryId,
        small_bcs_id: Option<ChangesetId>,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO synced_working_copy_equivalence (large_repo_id, large_bcs_id, small_repo_id, small_bcs_id) VALUES {values}"
    }

    read SelectWorkingCopyEquivalence(
        source_repo_id: RepositoryId,
        bcs_id: ChangesetId,
        target_repo_id: RepositoryId,
    ) -> (RepositoryId, ChangesetId, RepositoryId, Option<ChangesetId>) {
        "SELECT large_repo_id, large_bcs_id, small_repo_id, small_bcs_id
         FROM synced_working_copy_equivalence
         WHERE (large_repo_id = {source_repo_id} AND small_repo_id = {target_repo_id} AND large_bcs_id = {bcs_id})
         OR (large_repo_id = {target_repo_id} AND small_repo_id = {source_repo_id} AND small_bcs_id = {bcs_id})
         ORDER BY mapping_id ASC
         LIMIT 1
         "
    }
}

impl SqlConstructors for SqlSyncedCommitMapping {
    const LABEL: &'static str = "synced_commit_mapping";

    fn from_connections(
        write_connection: Connection,
        read_connection: Connection,
        read_master_connection: Connection,
    ) -> Self {
        Self {
            write_connection,
            read_connection,
            read_master_connection,
        }
    }

    fn get_up_query() -> &'static str {
        include_str!("../schemas/sqlite-synced-commit-mapping.sql")
    }
}

impl SqlSyncedCommitMapping {
    fn add_many_in_txn(
        &self,
        entries: Vec<SyncedCommitMappingEntry>,
        txn: Transaction,
    ) -> impl Future<Item = (Transaction, u64), Error = Error> {
        let insert_entries: Vec<_> = entries
            .iter()
            .map(|entry| {
                (
                    &entry.large_repo_id,
                    &entry.large_bcs_id,
                    &entry.small_repo_id,
                    &entry.small_bcs_id,
                )
            })
            .collect();

        InsertMapping::query_with_transaction(txn, &insert_entries).and_then(
            move |(txn, _result)| {
                let wces: Vec<_> = entries
                    .into_iter()
                    .map(|e| EquivalentWorkingCopyEntry {
                        large_repo_id: e.large_repo_id,
                        large_bcs_id: e.large_bcs_id,
                        small_repo_id: e.small_repo_id,
                        small_bcs_id: Some(e.small_bcs_id),
                    })
                    .collect();
                let wce_entries: Vec<_> = wces
                    .iter()
                    .map(|entry| {
                        (
                            &entry.large_repo_id,
                            &entry.large_bcs_id,
                            &entry.small_repo_id,
                            &entry.small_bcs_id,
                        )
                    })
                    .collect();
                InsertWorkingCopyEquivalence::query_with_transaction(txn, &wce_entries)
                    .map(|(txn, result)| (txn, result.affected_rows()))
            },
        )
    }

    fn add_many(
        &self,
        entries: Vec<SyncedCommitMappingEntry>,
    ) -> impl Future<Item = u64, Error = Error> {
        let this = self.clone();
        self.write_connection
            .start_transaction()
            .and_then(move |txn| {
                this.add_many_in_txn(entries, txn)
                    .and_then(|(txn, affected_rows)| txn.commit().map(move |()| affected_rows))
            })
    }
}

impl SyncedCommitMapping for SqlSyncedCommitMapping {
    fn add(&self, _ctx: CoreContext, entry: SyncedCommitMappingEntry) -> BoxFuture<bool, Error> {
        STATS::adds.add_value(1);

        self.add_many(vec![entry]).map(|count| count == 1).boxify()
    }

    fn add_no_commit(
        &self,
        _ctx: CoreContext,
        entry: SyncedCommitMappingEntry,
    ) -> BoxFuture<Transaction, Error> {
        STATS::add_no_commit.add_value(1);

        let this = self.clone();
        self.write_connection
            .start_transaction()
            .and_then(move |txn| this.add_many_in_txn(vec![entry], txn).map(|(txn, _)| txn))
            .boxify()
    }

    fn add_bulk(
        &self,
        _ctx: CoreContext,
        entries: Vec<SyncedCommitMappingEntry>,
    ) -> BoxFuture<u64, Error> {
        STATS::add_bulks.add_value(1);

        self.add_many(entries).boxify()
    }

    fn get(
        &self,
        _ctx: CoreContext,
        source_repo_id: RepositoryId,
        bcs_id: ChangesetId,
        target_repo_id: RepositoryId,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        STATS::gets.add_value(1);

        SelectMapping::query(
            &self.read_connection,
            &source_repo_id,
            &bcs_id,
            &target_repo_id,
        )
        .and_then({
            cloned!(self.read_master_connection);
            move |rows| {
                if rows.is_empty() {
                    STATS::gets_master.add_value(1);
                    SelectMapping::query(
                        &read_master_connection,
                        &source_repo_id,
                        &bcs_id,
                        &target_repo_id,
                    )
                    .left_future()
                } else {
                    future::ok(rows).right_future()
                }
            }
        })
        .map(move |rows| {
            if rows.len() == 1 {
                let (large_repo_id, large_bcs_id, _small_repo_id, small_bcs_id) = rows[0];
                if target_repo_id == large_repo_id {
                    Some(large_bcs_id)
                } else {
                    Some(small_bcs_id)
                }
            } else {
                None
            }
        })
        .boxify()
    }

    fn insert_equivalent_working_copy(
        &self,
        ctx: CoreContext,
        entry: EquivalentWorkingCopyEntry,
    ) -> BoxFuture<bool, Error> {
        STATS::insert_working_copy_eqivalence.add_value(1);

        let EquivalentWorkingCopyEntry {
            large_repo_id,
            large_bcs_id,
            small_repo_id,
            small_bcs_id,
        } = entry;

        let this = self.clone();
        InsertWorkingCopyEquivalence::query(
            &self.write_connection,
            &[(&large_repo_id, &large_bcs_id, &small_repo_id, &small_bcs_id)],
        )
        .and_then(move |result| {
            if result.affected_rows() == 1 {
                future::ok(true).left_future()
            } else {
                // Check that db stores consistent entry
                this.get_equivalent_working_copy(
                    ctx.clone(),
                    large_repo_id,
                    large_bcs_id,
                    small_repo_id,
                )
                .and_then(move |maybe_equivalent_wc| {
                    if let Some(equivalent_wc) = maybe_equivalent_wc {
                        use WorkingCopyEquivalence::*;
                        let expected_small_bcs_id = match equivalent_wc {
                            WorkingCopy(wc) => Some(wc),
                            NoWorkingCopy => None,
                        };

                        if expected_small_bcs_id != small_bcs_id {
                            let err = ErrorKind::InconsistentWorkingCopyEntry {
                                actual: small_bcs_id,
                                expected: expected_small_bcs_id,
                            };
                            return Err(err.into());
                        }
                    }
                    Ok(false)
                })
                .right_future()
            }
        })
        .boxify()
    }

    fn get_equivalent_working_copy(
        &self,
        _ctx: CoreContext,
        source_repo_id: RepositoryId,
        source_bcs_id: ChangesetId,
        target_repo_id: RepositoryId,
    ) -> BoxFuture<Option<WorkingCopyEquivalence>, Error> {
        STATS::get_equivalent_working_copy.add_value(1);

        cloned!(self.read_master_connection);
        SelectWorkingCopyEquivalence::query(
            &self.read_connection,
            &source_repo_id,
            &source_bcs_id,
            &target_repo_id,
        )
        .and_then(move |rows| {
            if rows.len() >= 1 {
                future::ok(rows.get(0).cloned()).left_future()
            } else {
                SelectWorkingCopyEquivalence::query(
                    &read_master_connection,
                    &source_repo_id,
                    &source_bcs_id,
                    &target_repo_id,
                )
                .map(|rows| rows.get(0).cloned())
                .right_future()
            }
        })
        .map(move |maybe_row| match maybe_row {
            Some(row) => {
                let (large_repo_id, large_bcs_id, _small_repo_id, maybe_small_bcs_id) = row;

                if target_repo_id == large_repo_id {
                    Some(WorkingCopyEquivalence::WorkingCopy(large_bcs_id))
                } else {
                    match maybe_small_bcs_id {
                        Some(small_bcs_id) => {
                            Some(WorkingCopyEquivalence::WorkingCopy(small_bcs_id))
                        }
                        None => Some(WorkingCopyEquivalence::NoWorkingCopy),
                    }
                }
            }
            None => None,
        })
        .boxify()
    }
}
