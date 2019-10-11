/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use abomonation_derive::Abomonation;
use bytes::Bytes;
use cloned::cloned;
use context::{CoreContext, PerfCounterType};
use fbthrift::compact_protocol;
use futures::{future::ok, stream, Future, IntoFuture};
use futures_ext::{try_boxfuture, BoxFuture, BoxStream, FutureExt, StreamExt};
use heapsize_derive::HeapSizeOf;
use mononoke_types::{ChangesetId, RepositoryId};
use sql::{queries, Connection, Transaction};
pub use sql_ext::SqlConstructors;
use stats::{define_stats, Timeseries};
use std::collections::{HashMap, HashSet};
use std::result;

mod caching;
mod errors;
#[cfg(test)]
mod test;
mod wrappers;

pub use caching::{get_cache_key, CachingChangesets};
pub use errors::*;

define_stats! {
    prefix = "mononoke.changesets";
    gets: timeseries(RATE, SUM),
    get_many: timeseries(RATE, SUM),
    gets_master: timeseries(RATE, SUM),
    get_many_master: timeseries(RATE, SUM),
    adds: timeseries(RATE, SUM),
}

#[derive(Abomonation, Clone, Debug, Eq, Hash, HeapSizeOf, PartialEq)]
pub struct ChangesetEntry {
    pub repo_id: RepositoryId,
    pub cs_id: ChangesetId,
    pub parents: Vec<ChangesetId>,
    pub gen: u64,
}

impl ChangesetEntry {
    fn from_thrift(thrift_entry: changeset_entry_thrift::ChangesetEntry) -> Result<Self> {
        let parents: Result<Vec<_>> = thrift_entry
            .parents
            .into_iter()
            .map(ChangesetId::from_thrift)
            .collect();

        Ok(Self {
            repo_id: RepositoryId::new(thrift_entry.repo_id.0),
            cs_id: ChangesetId::from_thrift(thrift_entry.cs_id)?,
            parents: parents?,
            gen: thrift_entry.gen.0 as u64,
        })
    }

    fn into_thrift(self) -> changeset_entry_thrift::ChangesetEntry {
        changeset_entry_thrift::ChangesetEntry {
            repo_id: changeset_entry_thrift::RepoId(self.repo_id.id()),
            cs_id: self.cs_id.into_thrift(),
            parents: self.parents.into_iter().map(|p| p.into_thrift()).collect(),
            gen: changeset_entry_thrift::GenerationNum(self.gen as i64),
        }
    }
}

pub fn serialize_cs_entries(cs_entries: Vec<ChangesetEntry>) -> Bytes {
    let mut thrift_entries = vec![];
    for entry in cs_entries {
        let thrift_entry = changeset_entry_thrift::ChangesetEntry {
            repo_id: changeset_entry_thrift::RepoId(entry.repo_id.id()),
            cs_id: entry.cs_id.into_thrift(),
            parents: entry.parents.into_iter().map(|p| p.into_thrift()).collect(),
            gen: changeset_entry_thrift::GenerationNum(entry.gen as i64),
        };
        thrift_entries.push(thrift_entry);
    }

    compact_protocol::serialize(&thrift_entries)
}

pub fn deserialize_cs_entries(blob: &Bytes) -> Result<Vec<ChangesetEntry>> {
    let thrift_entries: Vec<changeset_entry_thrift::ChangesetEntry> =
        compact_protocol::deserialize(blob)?;
    let mut entries = vec![];
    for thrift_entry in thrift_entries {
        let parents: Result<Vec<_>> = thrift_entry
            .parents
            .into_iter()
            .map(ChangesetId::from_thrift)
            .collect();

        let parents = parents?;
        let entry = ChangesetEntry {
            repo_id: RepositoryId::new(thrift_entry.repo_id.0),
            cs_id: ChangesetId::from_thrift(thrift_entry.cs_id)?,
            parents,
            gen: thrift_entry.gen.0 as u64,
        };
        entries.push(entry);
    }

    Ok(entries)
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ChangesetInsert {
    pub repo_id: RepositoryId,
    pub cs_id: ChangesetId,
    pub parents: Vec<ChangesetId>,
}

/// Interface to storage of changesets that have been completely stored in Mononoke.
pub trait Changesets: Send + Sync {
    /// Add a new entry to the changesets table. Returns true if new changeset was inserted,
    /// returns false if the same changeset has already existed.
    fn add(&self, ctx: CoreContext, cs: ChangesetInsert) -> BoxFuture<bool, Error>;

    /// Retrieve the row specified by this commit, if available.
    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<ChangesetEntry>, Error>;

    /// Retrieve the rows for all the commits if available
    fn get_many(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_ids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetEntry>, Error>;
}

#[derive(Clone)]
pub struct SqlChangesets {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
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

    read SelectChangeset(repo_id: RepositoryId, cs_id: ChangesetId) -> (u64, Option<ChangesetId>) {
        "SELECT cs.gen, pcs.cs_id
         FROM changesets cs
         LEFT JOIN (csparents p, changesets pcs)
         ON (cs.id = p.cs_id AND p.parent_id = pcs.id)
         WHERE cs.repo_id = {repo_id}
           AND cs.cs_id = {cs_id}
         ORDER BY p.seq ASC"
    }

    read SelectManyChangesets(repo_id: RepositoryId, >list cs_id: ChangesetId) -> (ChangesetId, u64, Option<ChangesetId>) {
        "SELECT cs.cs_id, cs.gen, pcs.cs_id
         FROM changesets cs
         LEFT JOIN (csparents p, changesets pcs)
         ON (cs.id = p.cs_id AND p.parent_id = pcs.id)
         WHERE cs.repo_id = {repo_id}
           AND cs.cs_id IN {cs_id}
         ORDER BY p.seq ASC"
    }

    read SelectChangesets(repo_id: RepositoryId, >list cs_id: ChangesetId) -> (u64, ChangesetId, u64) {
        "SELECT id, cs_id, gen
         FROM changesets
         WHERE repo_id = {repo_id}
           AND cs_id IN {cs_id}"
    }

    read SelectAllChangesetsIdsInRange(repo_id: RepositoryId, min_id: u64, max_id: u64) -> (ChangesetId) {
        "SELECT cs_id
         FROM changesets
         WHERE repo_id = {repo_id}
           AND id BETWEEN {min_id} AND {max_id}"
    }

    read SelectChangesetsIdsBounds(repo_id: RepositoryId) -> (u64, u64) {
        "SELECT min(id), max(id)
         FROM changesets
         WHERE repo_id = {repo_id}"
    }

}

impl SqlConstructors for SqlChangesets {
    const LABEL: &'static str = "changesets";

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
        include_str!("../schemas/sqlite-changesets.sql")
    }
}

impl Changesets for SqlChangesets {
    fn add(&self, ctx: CoreContext, cs: ChangesetInsert) -> BoxFuture<bool, Error> {
        STATS::adds.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        cloned!(self.write_connection);

        let parent_rows = {
            if cs.parents.is_empty() {
                Ok(Vec::new()).into_future().boxify()
            } else {
                SelectChangesets::query(&write_connection, &cs.repo_id, &cs.parents[..]).boxify()
            }
        };

        parent_rows
            .and_then(move |parent_rows| {
                try_boxfuture!(check_missing_rows(&cs.parents, &parent_rows));
                let gen = parent_rows.iter().map(|row| row.2).max().unwrap_or(0) + 1;
                write_connection
                    .start_transaction()
                    .and_then({
                        cloned!(cs);
                        move |transaction| {
                            InsertChangeset::query_with_transaction(
                                transaction,
                                &[(&cs.repo_id, &cs.cs_id, &gen)],
                            )
                        }
                    })
                    .and_then(move |(transaction, result)| {
                        if result.affected_rows() == 1 && result.last_insert_id().is_some() {
                            insert_parents(
                                transaction,
                                result.last_insert_id().unwrap(),
                                cs,
                                parent_rows,
                            )
                            .map(|()| true)
                            .left_future()
                        } else {
                            transaction
                                .rollback()
                                .and_then(move |()| check_changeset_matches(&write_connection, cs))
                                .map(|()| false)
                                .right_future()
                        }
                    })
                    .boxify()
            })
            .boxify()
    }

    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<ChangesetEntry>, Error> {
        STATS::gets.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        cloned!(self.read_master_connection);

        select_changeset(&self.read_connection, repo_id, cs_id)
            .and_then(move |maybe_mapping| match maybe_mapping {
                Some(mapping) => Ok(Some(mapping)).into_future().boxify(),
                None => {
                    STATS::gets_master.add_value(1);
                    ctx.perf_counters()
                        .increment_counter(PerfCounterType::SqlReadsMaster);
                    select_changeset(&read_master_connection, repo_id, cs_id)
                }
            })
            .boxify()
    }

    fn get_many(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_ids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetEntry>, Error> {
        cloned!(self.read_master_connection);

        if cs_ids.is_empty() {
            ok(vec![]).boxify()
        } else {
            STATS::get_many.add_value(1);
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsReplica);

            select_many_changesets(&self.read_connection, repo_id, &cs_ids)
                .and_then(move |fetched_cs| {
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
                        ok(fetched_cs).left_future()
                    } else {
                        STATS::get_many.add_value(1);
                        ctx.perf_counters()
                            .increment_counter(PerfCounterType::SqlReadsMaster);
                        select_many_changesets(&read_master_connection, repo_id, &notfetched_cs_ids)
                            .map(move |mut master_fetched_cs| {
                                master_fetched_cs.extend(fetched_cs);
                                master_fetched_cs
                            })
                            .right_future()
                    }
                })
                .boxify()
        }
    }
}

impl SqlChangesets {
    pub fn get_list_bs_cs_id_in_range(
        &self,
        repo_id: RepositoryId,
        min_id: u64,
        max_id: u64,
    ) -> BoxStream<ChangesetId, Error> {
        // [min_id, max_id)
        cloned!(self.read_master_connection);
        // As SQL request is BETWEEN, both bounds including
        let max_id = max_id - 1;

        SelectAllChangesetsIdsInRange::query(&read_master_connection, &repo_id, &min_id, &max_id)
            .map(move |rows| {
                let changesets_ids = rows.into_iter().map(|row| row.0);
                stream::iter_ok(changesets_ids).boxify()
            })
            .from_err()
            .flatten_stream()
            .boxify()
    }

    pub fn get_changesets_ids_bounds(
        &self,
        repo_id: RepositoryId,
    ) -> BoxFuture<(Option<u64>, Option<u64>), Error> {
        cloned!(self.read_master_connection);

        SelectChangesetsIdsBounds::query(&read_master_connection, &repo_id)
            .map(move |rows| {
                if rows.is_empty() {
                    (None, None)
                } else {
                    (Some(rows[0].0), Some(rows[0].1))
                }
            })
            .boxify()
    }
}

fn check_missing_rows(
    expected: &[ChangesetId],
    actual: &[(u64, ChangesetId, u64)],
) -> result::Result<(), ErrorKind> {
    // Could just count the number here and report an error if any are missing, but the reporting
    // wouldn't be as nice.
    let expected_set: HashSet<_> = expected.iter().collect();
    let actual_set: HashSet<_> = actual.iter().map(|row| &row.1).collect();
    let diff = &expected_set - &actual_set;
    if diff.is_empty() {
        Ok(())
    } else {
        Err(ErrorKind::MissingParents(
            diff.into_iter().map(|cs_id| *cs_id).collect(),
        ))
    }
}

fn insert_parents(
    transaction: Transaction,
    new_cs_id: u64,
    cs: ChangesetInsert,
    parent_rows: Vec<(u64, ChangesetId, u64)>,
) -> impl Future<Item = (), Error = Error> {
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
                .get(&parent)
                .expect("check_missing_rows check failed");

            (new_cs_id, *parent_id, seq)
        })
        .collect();

    let ref_parent_inserts: Vec<_> = parent_inserts
        .iter()
        .map(|row| (&row.0, &row.1, &row.2))
        .collect();

    InsertParents::query_with_transaction(transaction, &ref_parent_inserts[..])
        .and_then(|(transaction, _)| transaction.commit())
}

fn check_changeset_matches(
    connection: &Connection,
    cs: ChangesetInsert,
) -> impl Future<Item = (), Error = Error> {
    select_changeset(&connection, cs.repo_id, cs.cs_id).and_then(move |stored_cs| {
        let stored_parents = stored_cs.map(|cs| cs.parents);
        if Some(&cs.parents) == stored_parents.as_ref() {
            Ok(())
        } else {
            Err(ErrorKind::DuplicateInsertionInconsistency(
                cs.cs_id,
                stored_parents.unwrap_or(Vec::new()),
                cs.parents,
            )
            .into())
        }
    })
}

fn select_changeset(
    connection: &Connection,
    repo_id: RepositoryId,
    cs_id: ChangesetId,
) -> BoxFuture<Option<ChangesetEntry>, Error> {
    cloned!(repo_id, cs_id);

    SelectChangeset::query(&connection, &repo_id, &cs_id)
        .map(move |rows| {
            if rows.is_empty() {
                None
            } else {
                let gen = rows[0].0;
                Some(ChangesetEntry {
                    repo_id,
                    cs_id,
                    parents: rows.into_iter().filter_map(|row| row.1).collect(),
                    gen,
                })
            }
        })
        .boxify()
}

fn select_many_changesets(
    connection: &Connection,
    repo_id: RepositoryId,
    cs_ids: &Vec<ChangesetId>,
) -> impl Future<Item = Vec<ChangesetEntry>, Error = Error> {
    SelectManyChangesets::query(&connection, &repo_id, &cs_ids[..]).map(move |fetched_changesets| {
        let mut cs_id_to_cs_entry = HashMap::new();

        for (cs_id, gen, maybe_parent) in fetched_changesets {
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

        cs_id_to_cs_entry.values().cloned().collect()
    })
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn serialize_deserialize() {
        let entry = ChangesetEntry {
            repo_id: RepositoryId::new(0),
            cs_id: mononoke_types_mocks::changesetid::ONES_CSID,
            parents: vec![mononoke_types_mocks::changesetid::TWOS_CSID],
            gen: 2,
        };

        let res = deserialize_cs_entries(&serialize_cs_entries(vec![entry.clone(), entry.clone()]))
            .unwrap();
        assert_eq!(vec![entry.clone(), entry], res);
    }
}
