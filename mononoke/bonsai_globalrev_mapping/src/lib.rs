/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use sql::Connection;
pub use sql_ext::SqlConstructors;
use std::collections::HashSet;

use anyhow::Error;
use cloned::cloned;
use context::CoreContext;
use futures::future::Future;
use futures::{future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::Globalrev;
use mononoke_types::{BonsaiChangeset, ChangesetId, RepositoryId};
use slog::warn;
use sql::queries;
use std::sync::Arc;

mod errors;

pub use crate::errors::ErrorKind;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BonsaiGlobalrevMappingEntry {
    pub repo_id: RepositoryId,
    pub bcs_id: ChangesetId,
    pub globalrev: Globalrev,
}

impl BonsaiGlobalrevMappingEntry {
    pub fn new(repo_id: RepositoryId, bcs_id: ChangesetId, globalrev: Globalrev) -> Self {
        BonsaiGlobalrevMappingEntry {
            repo_id,
            bcs_id,
            globalrev,
        }
    }
}

pub enum BonsaisOrGlobalrevs {
    Bonsai(Vec<ChangesetId>),
    Globalrev(Vec<Globalrev>),
}

impl BonsaisOrGlobalrevs {
    pub fn is_empty(&self) -> bool {
        match self {
            BonsaisOrGlobalrevs::Bonsai(v) => v.is_empty(),
            BonsaisOrGlobalrevs::Globalrev(v) => v.is_empty(),
        }
    }
}

impl From<ChangesetId> for BonsaisOrGlobalrevs {
    fn from(cs_id: ChangesetId) -> Self {
        BonsaisOrGlobalrevs::Bonsai(vec![cs_id])
    }
}

impl From<Vec<ChangesetId>> for BonsaisOrGlobalrevs {
    fn from(cs_ids: Vec<ChangesetId>) -> Self {
        BonsaisOrGlobalrevs::Bonsai(cs_ids)
    }
}

impl From<Globalrev> for BonsaisOrGlobalrevs {
    fn from(rev: Globalrev) -> Self {
        BonsaisOrGlobalrevs::Globalrev(vec![rev])
    }
}

impl From<Vec<Globalrev>> for BonsaisOrGlobalrevs {
    fn from(revs: Vec<Globalrev>) -> Self {
        BonsaisOrGlobalrevs::Globalrev(revs)
    }
}

pub trait BonsaiGlobalrevMapping: Send + Sync {
    fn add(&self, entry: BonsaiGlobalrevMappingEntry) -> BoxFuture<bool, Error>;

    fn add_many(&self, entries: Vec<BonsaiGlobalrevMappingEntry>) -> BoxFuture<(), Error>;

    fn get(
        &self,
        repo_id: RepositoryId,
        field: BonsaisOrGlobalrevs,
    ) -> BoxFuture<Vec<BonsaiGlobalrevMappingEntry>, Error>;

    fn get_globalrev_from_bonsai(
        &self,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<Globalrev>, Error>;

    fn get_bonsai_from_globalrev(
        &self,
        repo_id: RepositoryId,
        globalrev: Globalrev,
    ) -> BoxFuture<Option<ChangesetId>, Error>;
}

impl BonsaiGlobalrevMapping for Arc<dyn BonsaiGlobalrevMapping> {
    fn add(&self, entry: BonsaiGlobalrevMappingEntry) -> BoxFuture<bool, Error> {
        (**self).add(entry)
    }

    fn add_many(&self, entries: Vec<BonsaiGlobalrevMappingEntry>) -> BoxFuture<(), Error> {
        (**self).add_many(entries)
    }

    fn get(
        &self,
        repo_id: RepositoryId,
        field: BonsaisOrGlobalrevs,
    ) -> BoxFuture<Vec<BonsaiGlobalrevMappingEntry>, Error> {
        (**self).get(repo_id, field)
    }

    fn get_globalrev_from_bonsai(
        &self,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<Globalrev>, Error> {
        (**self).get_globalrev_from_bonsai(repo_id, cs_id)
    }

    fn get_bonsai_from_globalrev(
        &self,
        repo_id: RepositoryId,
        globalrev: Globalrev,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        (**self).get_bonsai_from_globalrev(repo_id, globalrev)
    }
}

queries! {
    write InsertMapping(values: (
        repo_id: RepositoryId,
        bcs_id: ChangesetId,
        globalrev: Globalrev,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bonsai_globalrev_mapping (repo_id, bcs_id, globalrev) VALUES {values}"
    }

    read SelectMappingByBonsai(
        repo_id: RepositoryId,
        >list bcs_id: ChangesetId
    ) -> (ChangesetId, Globalrev) {
        "SELECT bcs_id, globalrev
         FROM bonsai_globalrev_mapping
         WHERE repo_id = {repo_id} AND bcs_id in {bcs_id}"
    }

    read SelectMappingByGlobalrev(
        repo_id: RepositoryId,
        >list globalrev: Globalrev
    ) -> (ChangesetId, Globalrev) {
        "SELECT bcs_id, globalrev
         FROM bonsai_globalrev_mapping
         WHERE repo_id = {repo_id} AND globalrev in {globalrev}"
    }
}

#[derive(Clone)]
pub struct SqlBonsaiGlobalrevMapping {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
}

impl SqlConstructors for SqlBonsaiGlobalrevMapping {
    const LABEL: &'static str = "bonsai_globalrev_mapping";

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
        include_str!("../schemas/sqlite-bonsai-globalrev-mapping.sql")
    }
}

impl BonsaiGlobalrevMapping for SqlBonsaiGlobalrevMapping {
    fn add(&self, entry: BonsaiGlobalrevMappingEntry) -> BoxFuture<bool, Error> {
        let BonsaiGlobalrevMappingEntry {
            repo_id,
            bcs_id,
            globalrev,
        } = entry;
        cloned!(self.read_master_connection);

        InsertMapping::query(&self.write_connection, &[(&repo_id, &bcs_id, &globalrev)])
            .and_then(move |result| {
                if result.affected_rows() == 1 {
                    Ok(true).into_future().boxify()
                } else {
                    select_mapping(
                        &read_master_connection,
                        repo_id,
                        &BonsaisOrGlobalrevs::Bonsai(vec![bcs_id]),
                    )
                    .and_then(move |mappings| match mappings.into_iter().next() {
                        Some(BonsaiGlobalrevMappingEntry {
                            repo_id,
                            bcs_id,
                            globalrev,
                        }) => {
                            if globalrev == entry.globalrev {
                                Ok(false)
                            } else {
                                Err(ErrorKind::ConflictingEntries(
                                    BonsaiGlobalrevMappingEntry {
                                        repo_id,
                                        bcs_id,
                                        globalrev,
                                    },
                                    entry,
                                )
                                .into())
                            }
                        }
                        None => Err(ErrorKind::RaceConditionWithDelete(entry).into()),
                    })
                    .map(move |_| false)
                    .boxify()
                }
            })
            .boxify()
    }

    fn add_many(&self, entries: Vec<BonsaiGlobalrevMappingEntry>) -> BoxFuture<(), Error> {
        let entries: Vec<_> = entries
            .iter()
            .map(
                |BonsaiGlobalrevMappingEntry {
                     repo_id,
                     bcs_id,
                     globalrev,
                 }| (repo_id, bcs_id, globalrev),
            )
            .collect();

        InsertMapping::query(&self.write_connection, &entries[..])
            .from_err()
            .map(|_| ())
            .boxify()
    }

    fn get(
        &self,
        repo_id: RepositoryId,
        objects: BonsaisOrGlobalrevs,
    ) -> BoxFuture<Vec<BonsaiGlobalrevMappingEntry>, Error> {
        cloned!(self.read_master_connection);

        select_mapping(&self.read_connection, repo_id, &objects)
            .and_then(move |mut mappings| {
                let left_to_fetch = filter_fetched_objects(objects, &mappings[..]);

                if left_to_fetch.is_empty() {
                    Ok(mappings).into_future().left_future()
                } else {
                    select_mapping(&read_master_connection, repo_id, &left_to_fetch)
                        .map(move |mut master_mappings| {
                            mappings.append(&mut master_mappings);
                            mappings
                        })
                        .right_future()
                }
            })
            .boxify()
    }

    fn get_globalrev_from_bonsai(
        &self,
        repo_id: RepositoryId,
        bcs_id: ChangesetId,
    ) -> BoxFuture<Option<Globalrev>, Error> {
        self.get(repo_id, BonsaisOrGlobalrevs::Bonsai(vec![bcs_id]))
            .map(|result| result.into_iter().next().map(|entry| entry.globalrev))
            .boxify()
    }

    fn get_bonsai_from_globalrev(
        &self,
        repo_id: RepositoryId,
        globalrev: Globalrev,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        self.get(repo_id, BonsaisOrGlobalrevs::Globalrev(vec![globalrev]))
            .map(|result| result.into_iter().next().map(|entry| entry.bcs_id))
            .boxify()
    }
}

fn filter_fetched_objects(
    objects: BonsaisOrGlobalrevs,
    mappings: &[BonsaiGlobalrevMappingEntry],
) -> BonsaisOrGlobalrevs {
    match objects {
        BonsaisOrGlobalrevs::Bonsai(cs_ids) => {
            let bcs_fetched: HashSet<_> = mappings.iter().map(|m| &m.bcs_id).collect();

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
            let globalrevs_fetched: HashSet<_> = mappings.iter().map(|m| &m.globalrev).collect();

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

fn select_mapping(
    connection: &Connection,
    repo_id: RepositoryId,
    objects: &BonsaisOrGlobalrevs,
) -> BoxFuture<Vec<BonsaiGlobalrevMappingEntry>, Error> {
    cloned!(repo_id, objects);
    if objects.is_empty() {
        return future::ok(vec![]).boxify();
    }

    let rows_fut = match objects {
        BonsaisOrGlobalrevs::Bonsai(bcs_ids) => {
            SelectMappingByBonsai::query(&connection, &repo_id, &bcs_ids[..]).left_future()
        }
        BonsaisOrGlobalrevs::Globalrev(globalrevs) => {
            SelectMappingByGlobalrev::query(&connection, &repo_id, &globalrevs[..]).right_future()
        }
    };

    rows_fut
        .map(move |rows| {
            rows.into_iter()
                .map(move |(bcs_id, globalrev)| BonsaiGlobalrevMappingEntry {
                    repo_id,
                    bcs_id,
                    globalrev,
                })
                .collect()
        })
        .boxify()
}

pub fn upload_globalrevs(
    ctx: CoreContext,
    repo_id: RepositoryId,
    globalrevs_store: Arc<dyn BonsaiGlobalrevMapping>,
    cs_ids: Vec<BonsaiChangeset>,
) -> BoxFuture<(), Error> {
    let mut entries = vec![];
    for bcs in cs_ids {
        match Globalrev::from_bcs(bcs.clone()) {
            Ok(globalrev) => {
                let entry =
                    BonsaiGlobalrevMappingEntry::new(repo_id, bcs.get_changeset_id(), globalrev);
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
    globalrevs_store.add_many(entries).boxify()
}
