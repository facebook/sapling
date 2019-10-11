/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use sql::Connection;
pub use sql_ext::SqlConstructors;

use cloned::cloned;
use context::CoreContext;
use failure::Fail;
use failure_ext::Error;
use futures::{future, stream, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use itertools::Itertools;
use mononoke_types::{ChangesetId, RepositoryId};
use sql::queries;
use stats::{define_stats, Timeseries};

const GET_MULTIPLE_CHUNK_SIZE: usize = 100;

#[derive(Debug, Eq, Fail, PartialEq)]
pub enum ErrorKind {
    #[fail(display = "find_first_synced_in_list cannot operate on an empty list")]
    FindFirstSyncedNeedsAtLeastOneChangeset,
}

// TODO(simonfar): Once we've proven the concept, we want to cache these
define_stats! {
    prefix = "mononoke.synced_commit_mapping";
    gets: timeseries(RATE, SUM),
    gets_master: timeseries(RATE, SUM),
    adds: timeseries(RATE, SUM),
    get_alls: timeseries(RATE, SUM),
    get_alls_master: timeseries(RATE, SUM),
    get_multiples: timeseries(RATE, SUM),
    find_fist_synceds: timeseries(RATE, SUM),
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

    /// Get all mapping entries for a given source commit
    fn get_all(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        bcs_id: ChangesetId,
    ) -> BoxFuture<BTreeMap<RepositoryId, ChangesetId>, Error>;

    /// Find the mapping entries for multiple commits and a target repo
    fn get_multiple(
        &self,
        ctx: CoreContext,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
        source_bcs_ids: &[ChangesetId],
    ) -> BoxFuture<Vec<Option<ChangesetId>>, Error>;

    /// Find the first among `changesets` in `source_repo_id`, which has an equivalent in `target_repo_id`
    /// and return that equivalent. Note that "first" in this context refers to the oder
    /// in the `changesets`, not to any changeset date/time property.
    fn find_first_synced_in_list(
        &self,
        ctx: CoreContext,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
        changesets: &[ChangesetId],
    ) -> BoxFuture<Option<(ChangesetId, ChangesetId)>, Error> {
        STATS::find_fist_synceds.add_value(1);
        if changesets.len() == 0 {
            return future::err(Error::from(
                ErrorKind::FindFirstSyncedNeedsAtLeastOneChangeset,
            ))
            .boxify();
        }
        let changesets_chunks = changesets.iter().chunks(GET_MULTIPLE_CHUNK_SIZE);
        let chunk_futures = changesets_chunks.into_iter().map(|changesets_chunk| {
            let changesets_chunk_vec: Vec<ChangesetId> =
                changesets_chunk.into_iter().cloned().collect();
            self.get_multiple(
                ctx.clone(),
                source_repo_id.clone(),
                target_repo_id.clone(),
                &changesets_chunk_vec[..],
            )
            .map({
                cloned!(changesets_chunk_vec);
                move |changeset_mappings| {
                    changeset_mappings
                        .iter()
                        .enumerate()
                        .find_map(|(pos, mapping)| {
                            mapping.map(|mapped_cs_id| {
                                (changesets_chunk_vec[pos].clone(), mapped_cs_id)
                            })
                        })
                }
            })
        });
        stream::futures_ordered(chunk_futures)
            .filter_map(|el| el)
            .into_future()
            .map(|(maybe_mapping, _rest_of_the_stream)| {
                maybe_mapping.map(|(cs_id, cs_id_ref)| (cs_id, cs_id_ref.clone()))
            })
            .map_err(|(e, _)| e)
            .boxify()
    }
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
    fn get_all(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        bcs_id: ChangesetId,
    ) -> BoxFuture<BTreeMap<RepositoryId, ChangesetId>, Error> {
        (**self).get_all(ctx, repo_id, bcs_id)
    }
    fn get_multiple(
        &self,
        ctx: CoreContext,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
        source_bcs_ids: &[ChangesetId],
    ) -> BoxFuture<Vec<Option<ChangesetId>>, Error> {
        (**self).get_multiple(ctx, source_repo_id, target_repo_id, source_bcs_ids)
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

    read SelectMultiple(
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
        >list cs_ids: ChangesetId
    ) -> (RepositoryId, ChangesetId, ChangesetId) {
        "SELECT large_repo_id, large_bcs_id, small_bcs_id
         FROM synced_commit_mapping
         WHERE (large_repo_id = {source_repo_id} AND small_repo_id = {target_repo_id} AND large_bcs_id IN {cs_ids}) OR
         (small_repo_id = {source_repo_id} AND large_repo_id = {target_repo_id} AND small_bcs_id IN {cs_ids})"
    }

    read SelectAllMapping(
        source_repo_id: RepositoryId,
        bcs_id: ChangesetId,
    ) -> (RepositoryId, ChangesetId, RepositoryId, ChangesetId) {
        "SELECT large_repo_id, large_bcs_id, small_repo_id, small_bcs_id
         FROM synced_commit_mapping
         WHERE (large_repo_id = {source_repo_id} AND large_bcs_id = {bcs_id}) OR
         (small_repo_id = {source_repo_id} AND small_bcs_id = {bcs_id})"
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

    fn get_all(
        &self,
        _ctx: CoreContext,
        repo_id: RepositoryId,
        bcs_id: ChangesetId,
    ) -> BoxFuture<BTreeMap<RepositoryId, ChangesetId>, Error> {
        STATS::get_alls.add_value(1);

        SelectAllMapping::query(&self.read_connection, &repo_id, &bcs_id)
            .and_then({
                cloned!(self.read_master_connection);
                move |rows| {
                    if rows.is_empty() {
                        STATS::get_alls_master.add_value(1);
                        SelectAllMapping::query(&read_master_connection, &repo_id, &bcs_id)
                            .left_future()
                    } else {
                        future::ok(rows).right_future()
                    }
                }
            })
            .map(move |rows| {
                rows.into_iter()
                    .map(
                        |(large_repo_id, large_bcs_id, small_repo_id, small_bcs_id)| {
                            if repo_id == large_repo_id {
                                (small_repo_id, small_bcs_id)
                            } else {
                                (large_repo_id, large_bcs_id)
                            }
                        },
                    )
                    .collect()
            })
            .boxify()
    }

    fn get_multiple(
        &self,
        _ctx: CoreContext,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
        source_bcs_ids: &[ChangesetId],
    ) -> BoxFuture<Vec<Option<ChangesetId>>, Error> {
        STATS::get_multiples.add_value(1);
        let source_bcs_ids_to_move: Vec<ChangesetId> = source_bcs_ids.iter().cloned().collect();
        SelectMultiple::query(
            &self.read_connection,
            &source_repo_id,
            &target_repo_id,
            &source_bcs_ids,
        )
        .map({
            move |rows| {
                let mapping1: HashMap<&ChangesetId, &ChangesetId> = rows
                    .iter()
                    .filter_map(|(large_repo_id, large_cs_id, small_cs_id)| {
                        // large_cs_id can be on the left side of
                        // the mapping only if large_repo is a source repo
                        if &source_repo_id == large_repo_id {
                            Some((large_cs_id, small_cs_id))
                        } else {
                            None
                        }
                    })
                    .collect();
                let mapping2: HashMap<&ChangesetId, &ChangesetId> = rows
                    .iter()
                    .filter_map(|(large_repo_id, large_cs_id, small_cs_id)| {
                        // small_cs_id can be on the left side of
                        // the mapping only if large_repo is a target repo
                        if &target_repo_id == large_repo_id {
                            Some((small_cs_id, large_cs_id))
                        } else {
                            None
                        }
                    })
                    .collect();
                let res: Vec<Option<ChangesetId>> = source_bcs_ids_to_move
                    .iter()
                    .map(|source_bcs_id| {
                        mapping1
                            .get(source_bcs_id)
                            .or_else(|| mapping2.get(source_bcs_id))
                            .cloned()
                            .cloned()
                    })
                    .collect();
                res
            }
        })
        .boxify()
    }
}
