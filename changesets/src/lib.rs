// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate abomonation;
#[macro_use]
extern crate abomonation_derive;
extern crate bytes;
extern crate cachelib;
#[macro_use]
extern crate cloned;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate heapsize;
#[macro_use]
extern crate heapsize_derive;
extern crate memcache;
#[macro_use]
extern crate sql;
extern crate sql_ext;
extern crate tokio;

extern crate changeset_entry_thrift;
#[macro_use]
extern crate futures_ext;
#[macro_use]
extern crate lazy_static;
extern crate mercurial_types;
extern crate mononoke_types;
#[cfg(test)]
extern crate mononoke_types_mocks;
extern crate rust_thrift;
#[macro_use]
extern crate stats;

use std::collections::{HashMap, HashSet};
use std::result;

use bytes::Bytes;
use failure::SyncFailure;

use sql::{Connection, Transaction};
pub use sql_ext::SqlConstructors;

use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::RepositoryId;
use mononoke_types::ChangesetId;
use rust_thrift::compact_protocol;
use stats::Timeseries;

mod caching;
mod errors;
mod wrappers;

pub use caching::{get_cache_key, CachingChangests};
pub use errors::*;

define_stats! {
    prefix = "mononoke.changesets";
    gets: timeseries(RATE, SUM),
    gets_master: timeseries(RATE, SUM),
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
        compact_protocol::deserialize(blob).map_err(SyncFailure::new)?;
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
    fn add(&self, cs: ChangesetInsert) -> BoxFuture<bool, Error>;

    /// Retrieve the row specified by this commit, if available.
    fn get(
        &self,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<ChangesetEntry>, Error>;
}

#[derive(Clone)]
pub struct SqlChangesets {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
}

queries!{
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

    read SelectChangesets(repo_id: RepositoryId, >list cs_id: ChangesetId) -> (u64, ChangesetId, u64) {
        "SELECT id, cs_id, gen
         FROM changesets
         WHERE repo_id = {repo_id}
           AND cs_id IN {cs_id}"
    }
}

impl SqlConstructors for SqlChangesets {
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
    fn add(&self, cs: ChangesetInsert) -> BoxFuture<bool, Error> {
        STATS::adds.add_value(1);
        cloned!(self.write_connection);

        SelectChangesets::query(
            &write_connection,
            &cs.repo_id,
            &cs.parents.iter().collect::<Vec<_>>()[..],
        ).and_then(move |parent_rows| {
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
                        ).map(|()| true)
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
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<ChangesetEntry>, Error> {
        STATS::gets.add_value(1);
        cloned!(self.read_master_connection);

        select_changeset(&self.read_connection, &repo_id, &cs_id)
            .and_then(move |maybe_mapping| match maybe_mapping {
                Some(mapping) => Ok(Some(mapping)).into_future().boxify(),
                None => {
                    STATS::gets_master.add_value(1);
                    select_changeset(&read_master_connection, &repo_id, &cs_id)
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
    select_changeset(&connection, &cs.repo_id, &cs.cs_id).and_then(move |stored_cs| {
        let stored_parents = stored_cs.map(|cs| cs.parents);
        if Some(&cs.parents) == stored_parents.as_ref() {
            Ok(())
        } else {
            Err(ErrorKind::DuplicateInsertionInconsistency(
                cs.cs_id,
                stored_parents.unwrap_or(Vec::new()),
                cs.parents,
            ).into())
        }
    })
}

fn select_changeset(
    connection: &Connection,
    repo_id: &RepositoryId,
    cs_id: &ChangesetId,
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
