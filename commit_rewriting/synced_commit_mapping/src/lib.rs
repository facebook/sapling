/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use std::sync::Arc;

use sql::Connection;
pub use sql_ext::SqlConstructors;

use cloned::cloned;
use context::CoreContext;
use failure_ext::Error;
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::{ChangesetId, RepositoryId};
use sql::queries;
use stats::{define_stats, Timeseries};

// TODO(simonfar): Once we've proven the concept, we want to cache these
define_stats! {
    prefix = "mononoke.synced_commit_mapping";
    gets: timeseries(RATE, SUM),
    gets_master: timeseries(RATE, SUM),
    adds: timeseries(RATE, SUM),
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

pub trait SyncedCommitMapping: Send + Sync {
    /// Given the full large, small mapping, store it in the DB.
    /// Future resolves to true if the mapping was saved, false otherwise
    fn add(&self, ctx: CoreContext, entry: SyncedCommitMappingEntry) -> BoxFuture<bool, Error>;

    /// Find the mapping entry for a given source commit and target repo
    fn get(
        &self,
        ctx: CoreContext,
        source_repo_id: RepositoryId,
        bcs_id: ChangesetId,
        target_repo_id: RepositoryId,
    ) -> BoxFuture<Option<ChangesetId>, Error>;
}

impl SyncedCommitMapping for Arc<dyn SyncedCommitMapping> {
    fn add(&self, ctx: CoreContext, entry: SyncedCommitMappingEntry) -> BoxFuture<bool, Error> {
        (**self).add(ctx, entry)
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

impl SyncedCommitMapping for SqlSyncedCommitMapping {
    fn add(&self, _ctx: CoreContext, entry: SyncedCommitMappingEntry) -> BoxFuture<bool, Error> {
        STATS::adds.add_value(1);

        let SyncedCommitMappingEntry {
            large_repo_id,
            large_bcs_id,
            small_repo_id,
            small_bcs_id,
        } = entry;

        InsertMapping::query(
            &self.write_connection,
            &[(&large_repo_id, &large_bcs_id, &small_repo_id, &small_bcs_id)],
        )
        .and_then(move |result| {
            if result.affected_rows() == 1 {
                Ok(true)
            } else {
                Ok(false)
            }
        })
        .boxify()
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
}
