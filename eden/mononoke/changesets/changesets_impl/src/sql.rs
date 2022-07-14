/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use changesets::ChangesetEntry;
use changesets::ChangesetInsert;
use changesets::Changesets;
use changesets::SortOrder;
use context::CoreContext;
use context::PerfCounterType;
use fbinit::FacebookInit;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::TryFutureExt;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::RepositoryId;
use rand::Rng;
use rendezvous::RendezVous;
use rendezvous::RendezVousOptions;
use rendezvous::RendezVousStats;
use rendezvous::TunablesRendezVousController;
use sql::queries;
use sql::Connection;
use sql::Transaction;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;
use stats::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use thiserror::Error;

define_stats! {
    prefix = "mononoke.changesets";
    gets: timeseries(Rate, Sum),
    gets_master: timeseries(Rate, Sum),
    get_many_by_prefix: timeseries(Rate, Sum),
    adds: timeseries(Rate, Sum),
}

#[derive(Debug, Eq, Error, PartialEq)]
pub enum SqlChangesetsError {
    #[error("Duplicate changeset {0} has different parents: {1:?} vs {2:?}")]
    DuplicateInsertionInconsistency(ChangesetId, Vec<ChangesetId>, Vec<ChangesetId>),
    #[error("Missing parents")]
    MissingParents(Vec<ChangesetId>),
}

#[derive(Clone)]
struct RendezVousConnection {
    rdv: RendezVous<ChangesetId, ChangesetEntry>,
    conn: Connection,
}

impl RendezVousConnection {
    fn new(conn: Connection, name: &str, opts: RendezVousOptions) -> Self {
        Self {
            conn,
            rdv: RendezVous::new(
                TunablesRendezVousController::new(opts),
                Arc::new(RendezVousStats::new(format!("changesets.{}", name,))),
            ),
        }
    }
}

#[derive(Clone)]
pub struct SqlChangesets {
    repo_id: RepositoryId,
    write_connection: Connection,
    read_connection: RendezVousConnection,
    read_master_connection: RendezVousConnection,
}

queries! {
    write InsertChangeset(values: (repo_id: RepositoryId, cs_id: ChangesetId, gen: u64)) {
        insert_or_ignore,
        "{insert_or_ignore} INTO changesets (repo_id, cs_id, gen) VALUES {values}"
    }

    write InsertParents(values: (cs_id: u64, parent_id: u64, seq: i32)) {
        none,
        "INSERT INTO csparents (cs_id, parent_id, seq) VALUES {values}"
    }

    read SelectChangeset(repo_id: RepositoryId, cs_id: ChangesetId, tok: i32) -> (u64, Option<ChangesetId>, Option<u64>, i32) {
        // NOTE: This selects seq even though we don't need it in order to sort by it.
        "
        SELECT cs0.gen AS gen, cs1.cs_id AS parent_id, csparents.seq AS seq, {tok}
        FROM csparents
        INNER JOIN changesets cs0 ON cs0.id = csparents.cs_id
        INNER JOIN changesets cs1 ON cs1.id = csparents.parent_id
        WHERE cs0.repo_id = {repo_id} AND cs0.cs_id = {cs_id} AND cs1.repo_id = {repo_id}

        UNION

        SELECT cs0.gen AS gen, NULL AS parent_id, NULL as seq, {tok}
        FROM changesets cs0
        WHERE cs0.repo_id = {repo_id} and cs0.cs_id = {cs_id}

        ORDER BY seq ASC
        "
    }

    read SelectManyChangesets(repo_id: RepositoryId, tok: i32, >list cs_id: ChangesetId) -> (ChangesetId, u64, Option<ChangesetId>, Option<u64>, i32) {
        "
        SELECT cs0.cs_id AS cs_id, cs0.gen AS gen, cs1.cs_id AS parent_id, csparents.seq AS seq, {tok}
        FROM csparents
        INNER JOIN changesets cs0 ON cs0.id = csparents.cs_id
        INNER JOIN changesets cs1 ON cs1.id = csparents.parent_id
        WHERE cs0.repo_id = {repo_id} AND cs0.cs_id IN {cs_id} AND cs1.repo_id = {repo_id}

        UNION

        SELECT cs0.cs_id AS cs_id, cs0.gen AS gen, NULL AS parent_id, NULL as seq, {tok}
        FROM changesets cs0
        WHERE cs0.repo_id = {repo_id} and cs0.cs_id IN {cs_id}

        ORDER BY seq ASC
        "
    }

    read SelectChangesets(repo_id: RepositoryId, >list cs_id: ChangesetId) -> (u64, ChangesetId, u64) {
        "SELECT id, cs_id, gen
         FROM changesets
         WHERE repo_id = {repo_id}
           AND cs_id IN {cs_id}"
    }

    read SelectChangesetsRange(repo_id: RepositoryId, min: &[u8], max: &[u8], limit: usize) -> (ChangesetId) {
        "SELECT cs_id
         FROM changesets
         WHERE repo_id = {repo_id}
           AND cs_id >= {min} AND cs_id <= {max}
           LIMIT {limit}
        "
    }

    read SelectAllChangesetsIdsInRange(repo_id: RepositoryId, min_id: u64, max_id: u64) -> (ChangesetId, u64) {
        mysql(
            "SELECT cs_id, id
            FROM changesets FORCE INDEX(repo_id_id)
            WHERE repo_id = {repo_id}
            AND id BETWEEN {min_id} AND {max_id}
            ORDER BY id"
        )
        sqlite(
            "SELECT cs_id, id
            FROM changesets
            WHERE repo_id = {repo_id}
            AND id BETWEEN {min_id} AND {max_id}
            ORDER BY id"
        )
    }

    read SelectAllChangesetsIdsInRangeLimitAsc(repo_id: RepositoryId, min_id: u64, max_id: u64, limit: u64) -> (ChangesetId, u64) {
        mysql(
            "SELECT cs_id, id
            FROM changesets FORCE INDEX(repo_id_id)
            WHERE repo_id = {repo_id}
            AND id BETWEEN {min_id} AND {max_id}
            ORDER BY id
            LIMIT {limit}"
        )
        sqlite(
            "SELECT cs_id, id
            FROM changesets
            WHERE repo_id = {repo_id}
            AND id BETWEEN {min_id} AND {max_id}
            ORDER BY id
            LIMIT {limit}"
        )
    }

    read SelectAllChangesetsIdsInRangeLimitDesc(repo_id: RepositoryId, min_id: u64, max_id: u64, limit: u64) -> (ChangesetId, u64) {
        mysql(
            "SELECT cs_id, id
            FROM changesets FORCE INDEX(repo_id_id)
            WHERE repo_id = {repo_id}
              AND id BETWEEN {min_id} AND {max_id}
            ORDER BY id DESC
            LIMIT {limit}"
        )
        sqlite(
            "SELECT cs_id, id
            FROM changesets
            WHERE repo_id = {repo_id}
              AND id BETWEEN {min_id} AND {max_id}
            ORDER BY id DESC
            LIMIT {limit}"
        )
    }

    read SelectChangesetsIdsBounds(repo_id: RepositoryId) -> (u64, u64) {
        "SELECT min(id), max(id)
         FROM changesets
         WHERE repo_id = {repo_id}"
    }

}

#[derive(Clone)]
pub struct SqlChangesetsBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlChangesetsBuilder {
    const LABEL: &'static str = "changesets";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-changesets.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlChangesetsBuilder {}

impl SqlChangesetsBuilder {
    pub fn build(self, opts: RendezVousOptions, repo_id: RepositoryId) -> SqlChangesets {
        let SqlConnections {
            read_connection,
            read_master_connection,
            write_connection,
        } = self.connections;

        SqlChangesets {
            repo_id,
            read_connection: RendezVousConnection::new(read_connection, "read", opts),
            read_master_connection: RendezVousConnection::new(
                read_master_connection,
                "read_master",
                opts,
            ),
            write_connection,
        }
    }
}

#[async_trait]
impl Changesets for SqlChangesets {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn add(&self, ctx: CoreContext, cs: ChangesetInsert) -> Result<bool, Error> {
        STATS::adds.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        let parent_rows = {
            if cs.parents.is_empty() {
                Vec::new()
            } else {
                SelectChangesets::query(&self.write_connection, &self.repo_id, &cs.parents[..])
                    .await?
            }
        };
        check_missing_rows(&cs.parents, &parent_rows)?;
        let gen = parent_rows.iter().map(|row| row.2).max().unwrap_or(0) + 1;
        let transaction = self.write_connection.start_transaction().await?;
        let (transaction, result) = InsertChangeset::query_with_transaction(
            transaction,
            &[(&self.repo_id, &cs.cs_id, &gen)],
        )
        .await?;

        if result.affected_rows() == 1 && result.last_insert_id().is_some() {
            insert_parents(
                transaction,
                result.last_insert_id().unwrap(),
                cs,
                parent_rows,
            )
            .await?;
            Ok(true)
        } else {
            transaction.rollback().await?;
            check_changeset_matches(&self.write_connection, self.repo_id, cs).await?;
            Ok(false)
        }
    }

    async fn get(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEntry>, Error> {
        let res = self.get_many(ctx, vec![cs_id]).await?.into_iter().next();
        Ok(res)
    }

    async fn get_many(
        &self,
        ctx: CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetEntry>, Error> {
        if cs_ids.is_empty() {
            return Ok(vec![]);
        }
        STATS::gets.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let fetched_cs =
            select_many_changesets(ctx.fb, &self.read_connection, self.repo_id, &cs_ids).await?;
        let fetched_set: HashSet<_> = fetched_cs
            .clone()
            .into_iter()
            .map(|cs_entry| cs_entry.cs_id)
            .collect();

        let notfetched_cs_ids: Vec<_> = cs_ids
            .into_iter()
            .filter(|cs_id| !fetched_set.contains(cs_id))
            .collect();
        if notfetched_cs_ids.is_empty() {
            Ok(fetched_cs)
        } else {
            STATS::gets_master.add_value(1);
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            let mut master_fetched_cs = select_many_changesets(
                ctx.fb,
                &self.read_master_connection,
                self.repo_id,
                &notfetched_cs_ids,
            )
            .await?;
            master_fetched_cs.extend(fetched_cs);
            Ok(master_fetched_cs)
        }
    }

    async fn get_many_by_prefix(
        &self,
        ctx: CoreContext,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix, Error> {
        STATS::get_many_by_prefix.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let resolved_cs =
            fetch_many_by_prefix(&self.read_connection.conn, self.repo_id, &cs_prefix, limit)
                .await?;
        match resolved_cs {
            ChangesetIdsResolvedFromPrefix::NoMatch => {
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsMaster);
                fetch_many_by_prefix(
                    &self.read_master_connection.conn,
                    self.repo_id,
                    &cs_prefix,
                    limit,
                )
                .await
            }
            _ => Ok(resolved_cs),
        }
    }

    fn prime_cache(&self, _ctx: &CoreContext, _changesets: &[ChangesetEntry]) {
        // No-op
    }

    async fn enumeration_bounds(
        &self,
        _ctx: &CoreContext,
        read_from_master: bool,
        known_heads: Vec<ChangesetId>,
    ) -> Result<Option<(u64, u64)>, Error> {
        let conn = self.read_conn(read_from_master);
        let rows = SelectChangesetsIdsBounds::query(conn, &self.repo_id).await?;
        if rows.is_empty() {
            Ok(None)
        } else {
            let (mut lo, hi) = (rows[0].0, rows[0].1);
            if !known_heads.is_empty() {
                let rows = SelectChangesets::query(conn, &self.repo_id, &known_heads).await?;
                let max_id = rows
                    .into_iter()
                    .map(|(id, _cs, _gen)| id)
                    .max()
                    // We want to skip the commits we've been given
                    .map_or(lo, |i| i + 1);
                lo = lo.max(max_id);
            }
            Ok(Some((lo, hi)))
        }
    }

    fn list_enumeration_range(
        &self,
        _ctx: &CoreContext,
        min_id: u64,
        max_id: u64,
        sort_and_limit: Option<(SortOrder, u64)>,
        read_from_master: bool,
    ) -> BoxStream<'_, Result<(ChangesetId, u64), Error>> {
        // We expect the range [min_id, max_id), so subtract 1 from max_id as
        // SQL request is BETWEEN, which means both bounds are inclusive.
        let max_id = max_id - 1;
        let conn = self.read_conn(read_from_master);

        async move {
            match sort_and_limit {
                None => {
                    SelectAllChangesetsIdsInRange::query(conn, &self.repo_id, &min_id, &max_id)
                        .await
                }
                Some((SortOrder::Ascending, limit)) => {
                    SelectAllChangesetsIdsInRangeLimitAsc::query(
                        conn,
                        &self.repo_id,
                        &min_id,
                        &max_id,
                        &limit,
                    )
                    .await
                }
                Some((SortOrder::Descending, limit)) => {
                    SelectAllChangesetsIdsInRangeLimitDesc::query(
                        conn,
                        &self.repo_id,
                        &min_id,
                        &max_id,
                        &limit,
                    )
                    .await
                }
            }
        }
        .map_ok(|rows| {
            let changesets_ids = rows.into_iter().map(|row| Ok((row.0, row.1)));
            stream::iter(changesets_ids)
        })
        .try_flatten_stream()
        .boxed()
    }
}

async fn fetch_many_by_prefix(
    connection: &Connection,
    repo_id: RepositoryId,
    cs_prefix: &ChangesetIdPrefix,
    limit: usize,
) -> Result<ChangesetIdsResolvedFromPrefix, Error> {
    let rows = SelectChangesetsRange::query(
        connection,
        &repo_id,
        &cs_prefix.min_as_ref(),
        &cs_prefix.max_as_ref(),
        &(limit + 1),
    )
    .await?;
    let mut fetched_cs: Vec<ChangesetId> = rows.into_iter().map(|row| row.0).collect();
    let result = match fetched_cs.len() {
        0 => ChangesetIdsResolvedFromPrefix::NoMatch,
        1 => ChangesetIdsResolvedFromPrefix::Single(fetched_cs[0].clone()),
        l if l <= limit => ChangesetIdsResolvedFromPrefix::Multiple(fetched_cs),
        _ => ChangesetIdsResolvedFromPrefix::TooMany({
            fetched_cs.pop();
            fetched_cs
        }),
    };
    Ok(result)
}

impl SqlChangesets {
    fn read_conn(&self, read_from_master: bool) -> &Connection {
        if read_from_master {
            &self.read_master_connection.conn
        } else {
            &self.read_connection.conn
        }
    }
}

fn check_missing_rows(
    expected: &[ChangesetId],
    actual: &[(u64, ChangesetId, u64)],
) -> Result<(), SqlChangesetsError> {
    // Could just count the number here and report an error if any are missing, but the reporting
    // wouldn't be as nice.
    let expected_set: HashSet<_> = expected.iter().collect();
    let actual_set: HashSet<_> = actual.iter().map(|row| &row.1).collect();
    let diff = &expected_set - &actual_set;
    if diff.is_empty() {
        Ok(())
    } else {
        Err(SqlChangesetsError::MissingParents(
            diff.into_iter().copied().collect(),
        ))
    }
}

async fn insert_parents(
    transaction: Transaction,
    new_cs_id: u64,
    cs: ChangesetInsert,
    parent_rows: Vec<(u64, ChangesetId, u64)>,
) -> Result<(), Error> {
    // parent_rows might not be in the same order as cs.parents.
    let parent_map: HashMap<_, _> = parent_rows.into_iter().map(|row| (row.1, row.0)).collect();

    // enumerate() would be OK here too, but involve conversions from usize
    // to i32 within the map function.
    let parent_inserts: Vec<_> = (0..(cs.parents.len() as i32))
        .zip(cs.parents.iter())
        .map(|(seq, parent)| {
            // check_missing_rows should have ensured that all the IDs are
            // present.
            let parent_id = parent_map
                .get(parent)
                .expect("check_missing_rows check failed");

            (new_cs_id, *parent_id, seq)
        })
        .collect();

    let ref_parent_inserts: Vec<_> = parent_inserts
        .iter()
        .map(|row| (&row.0, &row.1, &row.2))
        .collect();

    let (transaction, _) =
        InsertParents::query_with_transaction(transaction, &ref_parent_inserts[..]).await?;
    transaction.commit().await?;
    Ok(())
}

async fn check_changeset_matches(
    connection: &Connection,
    repo_id: RepositoryId,
    cs: ChangesetInsert,
) -> Result<(), Error> {
    let stored_parents = select_changeset(connection, repo_id, cs.cs_id)
        .await?
        .map(|cs| cs.parents);
    if Some(&cs.parents) == stored_parents.as_ref() {
        Ok(())
    } else {
        Err(SqlChangesetsError::DuplicateInsertionInconsistency(
            cs.cs_id,
            stored_parents.unwrap_or_default(),
            cs.parents,
        )
        .into())
    }
}

async fn select_changeset(
    connection: &Connection,
    repo_id: RepositoryId,
    cs_id: ChangesetId,
) -> Result<Option<ChangesetEntry>, Error> {
    let tok: i32 = rand::thread_rng().gen();
    let rows = SelectChangeset::query(connection, &repo_id, &cs_id, &tok).await?;
    let result = if rows.is_empty() {
        None
    } else {
        let gen = rows[0].0;
        Some(ChangesetEntry {
            repo_id,
            cs_id,
            parents: rows.into_iter().filter_map(|row| row.1).collect(),
            gen,
        })
    };
    Ok(result)
}

async fn select_many_changesets(
    fb: FacebookInit,
    connection: &RendezVousConnection,
    repo_id: RepositoryId,
    cs_ids: &[ChangesetId],
) -> Result<Vec<ChangesetEntry>, Error> {
    if cs_ids.is_empty() {
        return Ok(vec![]);
    }

    let ret = connection
        .rdv
        .dispatch(fb, cs_ids.iter().copied().collect(), || {
            let conn = connection.conn.clone();
            move |cs_ids| async move {
                let cs_ids = cs_ids.into_iter().collect::<Vec<_>>();

                let tok: i32 = rand::thread_rng().gen();

                let fetched_changesets =
                    SelectManyChangesets::query(&conn, &repo_id, &tok, &cs_ids[..]).await?;

                let mut cs_id_to_cs_entry = HashMap::new();
                for (cs_id, gen, maybe_parent, _, _) in fetched_changesets {
                    cs_id_to_cs_entry
                        .entry(cs_id)
                        .or_insert(ChangesetEntry {
                            repo_id,
                            cs_id,
                            parents: vec![],
                            gen,
                        })
                        .parents
                        .extend(maybe_parent.into_iter());
                }

                Ok(cs_id_to_cs_entry)
            }
        })
        .await?;

    Ok(ret.into_iter().filter_map(|(_, v)| v).collect())
}
