/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashSet;

use ::sql::Connection;
use ::sql::Transaction;
use ::sql_ext::mononoke_queries;
use anyhow::Error;
use async_trait::async_trait;
use context::CoreContext;
use context::PerfCounterType;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::Globalrev;
use mononoke_types::RepositoryId;
use slog::warn;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;
use thiserror::Error;

use super::BonsaiGlobalrevMapping;
use super::BonsaiGlobalrevMappingCacheEntry;
use super::BonsaiGlobalrevMappingEntries;
use super::BonsaiGlobalrevMappingEntry;
use super::BonsaisOrGlobalrevs;

mononoke_queries! {
    write DangerouslyAddGlobalrevs(values: (
        repo_id: RepositoryId,
        bcs_id: ChangesetId,
        globalrev: Globalrev,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bonsai_globalrev_mapping (repo_id, bcs_id, globalrev) VALUES {values}"
    }

    write ReplaceGlobalrevs(values: (
        repo_id: RepositoryId,
        bcs_id: ChangesetId,
        globalrev: Globalrev,
    )) {
        none,
        "REPLACE INTO bonsai_globalrev_mapping (repo_id, bcs_id, globalrev) VALUES {values}"
    }

    read SelectMappingByBonsai(
        repo_id: RepositoryId,
        >list bcs_id: ChangesetId
    ) -> (ChangesetId, Globalrev) {
        "SELECT bcs_id, globalrev
         FROM bonsai_globalrev_mapping
         WHERE repo_id = {repo_id} AND bcs_id in {bcs_id}"
    }

    read SelectMappingByGlobalrevCacheFriendly(
        repo_id: RepositoryId,
        max_globalrev: Globalrev,
        >list globalrev: Globalrev
    ) -> (ChangesetId, Globalrev) {
        "SELECT bcs_id, globalrev
         FROM bonsai_globalrev_mapping
         WHERE repo_id = {repo_id} AND globalrev in {globalrev}
         UNION ALL
         SELECT * FROM (
            SELECT bcs_id, globalrev
            FROM bonsai_globalrev_mapping
            WHERE repo_id = {repo_id} AND globalrev > {max_globalrev} LIMIT 1
        ) AS extra_for_negative_caching
        "
    }

    read SelectMaxEntry(repo_id: RepositoryId) -> (Globalrev,) {
        "
        SELECT globalrev
        FROM bonsai_globalrev_mapping
        WHERE repo_id = {repo_id}
        ORDER BY globalrev DESC
        LIMIT 1
        "
    }

    read SelectClosestGlobalrev(repo_id: RepositoryId, rev: Globalrev) -> (Globalrev,) {
        "
        SELECT globalrev
        FROM bonsai_globalrev_mapping
        WHERE repo_id = {repo_id} AND globalrev <= {rev}
        ORDER BY globalrev DESC
        LIMIT 1
        "
    }
}

impl BonsaiGlobalrevMappingEntries {
    pub fn empty() -> Self {
        Self {
            cached_data: Vec::new(),
        }
    }
    /// Construct a `BonsaiGlobalrevMappingEntries` instance from the outcome of a db query.
    /// This is where we perform the core logic of our negative caching trick:
    ///
    /// ## Context
    /// For a given repo, any globalrev may not have an associated bcs_id.
    /// It can happen for two reasons:
    /// * The change is recent (or future) and the table wasn't yet populated
    /// * The change doesn't have an associated globalrev and never will (globalrev gap)
    ///   This situation was always possible but has become frequent in the case of one of our
    ///   small-repos since the large-repo became the source-of-truth if the cross-repo config.
    ///   That is because the large-repo commits take up assigned globalrev slots that the small-repo commits
    ///   can't claim.
    /// In the former case, we should not cache a negative result.
    /// In the latter case, we should as it won't ever change.
    ///
    /// ## The trick
    /// When we query the globalrevs, we query the values we are interested in AND one extra value
    /// that is outside of the range we are interested in, see `SelectMappingByGlobalrevCacheFriendly` above.
    /// If a globalrev exists that is more recent (greater) than the max globalrev in our query,
    /// we know for sure that we are in the latter case above and the gap is forever, so it
    /// is safe to cache.
    /// See [this workplace thread for more context](https://fburl.com/workplace/4ulske1w)
    fn from_db_query(
        repo_id: RepositoryId,
        objects: &BonsaisOrGlobalrevs,
        fetched_data: Vec<(ChangesetId, Globalrev)>,
    ) -> Self {
        match objects {
            // The query given bonsais is exactly what you would think:
            // It returns each fetched datum without any further processing
            BonsaisOrGlobalrevs::Bonsai(_bonsais) => {
                let cached_data = fetched_data
                    .into_iter()
                    .map(|(bcs_id, globalrev)| BonsaiGlobalrevMappingCacheEntry {
                        repo_id,
                        bcs_id: Some(bcs_id),
                        globalrev,
                    })
                    .collect();
                Self { cached_data }
            }
            // The query given globalrevs includes a sentinel value after the range we are looking
            // for. This allows us to distinguish between a mapping that doesn't exist yet and
            // could exist later, and a mapping which will never exist, so that the absence of
            // mapping is safe to cache.
            BonsaisOrGlobalrevs::Globalrev(globalrevs) => {
                let mapping = fetched_data
                    .into_iter()
                    .map(|(bcs_id, globalrev)| (globalrev, bcs_id))
                    .collect::<BTreeMap<_, _>>();
                // If this is empty, it means we fetched no data so the max doesn't really matter
                let cached_data =
                    if let Some(max_globalrev_mapping) = mapping.last_key_value().map(|(k, _)| k) {
                        globalrevs
                            .iter()
                            .filter_map(|globalrev| {
                                if globalrev > max_globalrev_mapping {
                                    // We got no data for this globalrev or any subsequent globalrev from
                                    // the query, so we shouldn't cache it as the query includes an extra
                                    // sentinel value, which means that there should always be a value
                                    // here if we are not querying outside the range that this db replica
                                    // is aware of.
                                    None
                                } else {
                                    Some(BonsaiGlobalrevMappingCacheEntry {
                                        repo_id,
                                        bcs_id: mapping.get(globalrev).copied(),
                                        globalrev: *globalrev,
                                    })
                                }
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                Self { cached_data }
            }
        }
    }
    /// This is used in case the first query (from a replica db) hasn't returned all the
    /// information it could have.
    /// We will query the master db for what's left to fetch.
    fn left_to_fetch(&self, objects: BonsaisOrGlobalrevs) -> BonsaisOrGlobalrevs {
        match objects {
            BonsaisOrGlobalrevs::Bonsai(cs_ids) => {
                let bcs_fetched: HashSet<_> =
                    self.cached_data.iter().filter_map(|m| m.bcs_id).collect();

                BonsaisOrGlobalrevs::Bonsai(
                    cs_ids
                        .iter()
                        .filter_map(|cs| {
                            if !bcs_fetched.contains(cs) {
                                Some(*cs)
                            } else {
                                None
                            }
                        })
                        .collect(),
                )
            }
            BonsaisOrGlobalrevs::Globalrev(globalrevs) => {
                let globalrevs_fetched: HashSet<_> =
                    self.cached_data.iter().map(|m| &m.globalrev).collect();

                BonsaisOrGlobalrevs::Globalrev(
                    globalrevs
                        .iter()
                        .filter_map(|globalrev| {
                            if !globalrevs_fetched.contains(globalrev) {
                                Some(*globalrev)
                            } else {
                                None
                            }
                        })
                        .collect(),
                )
            }
        }
    }
    /// This is used to append the outcome of the query from the master db in cases where the
    /// replica db didn't contain all the expected results: `Self::left_to_fetch` returned a
    /// non-empty set.
    pub(crate) fn append(&mut self, mut other: Self) -> Self {
        let cached_data = &mut self.cached_data;
        cached_data.append(&mut other.cached_data);
        Self {
            cached_data: cached_data.to_vec(),
        }
    }
}

pub struct SqlBonsaiGlobalrevMapping {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

#[derive(Clone)]
pub struct SqlBonsaiGlobalrevMappingBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlBonsaiGlobalrevMappingBuilder {
    const LABEL: &'static str = "bonsai_globalrev_mapping";

    const CREATION_QUERY: &'static str =
        include_str!("../schemas/sqlite-bonsai-globalrev-mapping.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlBonsaiGlobalrevMappingBuilder {}

impl SqlBonsaiGlobalrevMappingBuilder {
    pub fn build(self, repo_id: RepositoryId) -> SqlBonsaiGlobalrevMapping {
        SqlBonsaiGlobalrevMapping {
            connections: self.connections,
            repo_id,
        }
    }
}

#[async_trait]
impl BonsaiGlobalrevMapping for SqlBonsaiGlobalrevMapping {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn bulk_import(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGlobalrevMappingEntry],
    ) -> Result<(), Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        let repo_id = self.repo_id;

        let entries: Vec<_> = entries
            .iter()
            .map(|entry| (&repo_id, &entry.bcs_id, &entry.globalrev))
            .collect();

        DangerouslyAddGlobalrevs::query(&self.connections.write_connection, &entries[..]).await?;

        Ok(())
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        objects: BonsaisOrGlobalrevs,
    ) -> Result<BonsaiGlobalrevMappingEntries, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let mut mappings =
            select_mapping(&self.connections.read_connection, self.repo_id, &objects).await?;

        let left_to_fetch = mappings.left_to_fetch(objects);

        if left_to_fetch.is_empty() {
            return Ok(mappings);
        }

        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);

        let master_mappings = select_mapping(
            &self.connections.read_master_connection,
            self.repo_id,
            &left_to_fetch,
        )
        .await?;
        mappings.append(master_mappings);
        Ok(mappings)
    }

    async fn get_closest_globalrev(
        &self,
        ctx: &CoreContext,
        globalrev: Globalrev,
    ) -> Result<Option<Globalrev>, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let row = SelectClosestGlobalrev::query(
            &self.connections.read_connection,
            &self.repo_id,
            &globalrev,
        )
        .await?
        .into_iter()
        .next();

        Ok(row.map(|r| r.0))
    }

    async fn get_max(&self, ctx: &CoreContext) -> Result<Option<Globalrev>, Error> {
        self.get_max_custom_repo(ctx, &self.repo_id).await
    }

    async fn get_max_custom_repo(
        &self,
        ctx: &CoreContext,
        repo_id: &RepositoryId,
    ) -> Result<Option<Globalrev>, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);

        let row = SelectMaxEntry::query(&self.connections.read_master_connection, repo_id)
            .await?
            .into_iter()
            .next();

        Ok(row.map(|r| r.0))
    }
}

async fn select_mapping(
    connection: &Connection,
    repo_id: RepositoryId,
    objects: &BonsaisOrGlobalrevs,
) -> Result<BonsaiGlobalrevMappingEntries, Error> {
    if objects.is_empty() {
        return Ok(BonsaiGlobalrevMappingEntries::empty());
    }

    let rows = match objects {
        BonsaisOrGlobalrevs::Bonsai(bcs_ids) => {
            SelectMappingByBonsai::query(connection, &repo_id, &bcs_ids[..]).await?
        }
        BonsaisOrGlobalrevs::Globalrev(globalrevs) => {
            let max_globalrev = globalrevs
                .iter()
                .max()
                .expect("We already returned earlier if objects.is_empty()");
            SelectMappingByGlobalrevCacheFriendly::query(
                connection,
                &repo_id,
                max_globalrev,
                &globalrevs[..],
            )
            .await?
        }
    };

    Ok(BonsaiGlobalrevMappingEntries::from_db_query(
        repo_id, objects, rows,
    ))
}

/// This method is for importing Globalrevs in bulk from a set of BonsaiChangesets where you know
/// they are correct. Don't use this to assign new Globalrevs.
pub async fn bulk_import_globalrevs<'a>(
    ctx: &'a CoreContext,
    globalrevs_store: &'a dyn BonsaiGlobalrevMapping,
    changesets: impl IntoIterator<Item = &'a BonsaiChangeset>,
) -> Result<(), Error> {
    let mut entries = vec![];
    for bcs in changesets.into_iter() {
        match Globalrev::from_bcs(bcs) {
            Ok(globalrev) => {
                let entry = BonsaiGlobalrevMappingEntry::new(bcs.get_changeset_id(), globalrev);
                entries.push(entry);
            }
            Err(e) => {
                warn!(
                    ctx.logger(),
                    "Couldn't fetch globalrev from commit: {:?}", e
                );
            }
        }
    }

    globalrevs_store.bulk_import(ctx, &entries).await?;

    Ok(())
}

#[derive(Debug, Error)]
pub enum AddGlobalrevsErrorKind {
    #[error("Conflict detected while inserting Globalrevs")]
    Conflict,

    #[error("Internal error occurred while inserting Globalrevs")]
    InternalError(#[from] Error),
}

// NOTE: For now, this is a top-level function since it doesn't use the connections in the
// SqlBonsaiGlobalrevMapping, but if we were to add more implementations of the
// BonsaiGlobalrevMapping trait, we should probably rethink the design of it, and not actually have
// it contain any connections (instead, they should be passed on by callers).
pub async fn add_globalrevs(
    transaction: Transaction,
    repo_id: RepositoryId,
    entries: impl IntoIterator<Item = &BonsaiGlobalrevMappingEntry>,
) -> Result<Transaction, AddGlobalrevsErrorKind> {
    let rows: Vec<_> = entries
        .into_iter()
        .map(|BonsaiGlobalrevMappingEntry { bcs_id, globalrev }| (&repo_id, bcs_id, globalrev))
        .collect();

    // It'd be really nice if we could rely on the error from an index conflict here, but our SQL
    // crate doesn't allow us to reach into this yet, so for now we check the number of affected
    // rows.

    let (transaction, res) =
        DangerouslyAddGlobalrevs::query_with_transaction(transaction, &rows[..]).await?;

    if res.affected_rows() != rows.len() as u64 {
        return Err(AddGlobalrevsErrorKind::Conflict);
    }

    Ok(transaction)
}
