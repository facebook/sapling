// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate abomonation;
#[macro_use]
extern crate abomonation_derive;
extern crate bonsai_hg_mapping_entry_thrift;
extern crate cachelib;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate heapsize;
#[macro_use]
extern crate heapsize_derive;
extern crate memcache;
extern crate tokio;

#[macro_use]
extern crate cloned;
extern crate futures_ext;
#[macro_use]
extern crate lazy_static;
extern crate mercurial_types;
extern crate mononoke_types;
extern crate rust_thrift;
#[macro_use]
extern crate sql;
extern crate sql_ext;
#[macro_use]
extern crate stats;

use std::sync::Arc;

use sql::Connection;
pub use sql_ext::SqlConstructors;

use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::{HgChangesetId, HgNodeHash, RepositoryId};
use mononoke_types::ChangesetId;
use stats::Timeseries;

mod caching;
mod errors;

pub use caching::CachingBonsaiHgMapping;
pub use errors::*;

define_stats! {
    prefix = "mononoke.bonsai-hg-mapping";
    gets: timeseries(RATE, SUM),
    gets_master: timeseries(RATE, SUM),
    adds: timeseries(RATE, SUM),
}

#[derive(Abomonation, Clone, Debug, Eq, Hash, HeapSizeOf, PartialEq)]
pub struct BonsaiHgMappingEntry {
    pub repo_id: RepositoryId,
    pub hg_cs_id: HgChangesetId,
    pub bcs_id: ChangesetId,
}

impl BonsaiHgMappingEntry {
    fn from_thrift(entry: bonsai_hg_mapping_entry_thrift::BonsaiHgMappingEntry) -> Result<Self> {
        Ok(Self {
            repo_id: RepositoryId::new(entry.repo_id.0),
            hg_cs_id: HgChangesetId::new(HgNodeHash::from_thrift(entry.hg_cs_id)?),
            bcs_id: ChangesetId::from_thrift(entry.bcs_id)?,
        })
    }

    fn into_thrift(self) -> bonsai_hg_mapping_entry_thrift::BonsaiHgMappingEntry {
        bonsai_hg_mapping_entry_thrift::BonsaiHgMappingEntry {
            repo_id: bonsai_hg_mapping_entry_thrift::RepoId(self.repo_id.id()),
            hg_cs_id: self.hg_cs_id.into_nodehash().into_thrift(),
            bcs_id: self.bcs_id.into_thrift(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, HeapSizeOf)]
pub enum BonsaiOrHgChangesetId {
    Bonsai(ChangesetId),
    Hg(HgChangesetId),
}

impl From<ChangesetId> for BonsaiOrHgChangesetId {
    fn from(cs_id: ChangesetId) -> Self {
        BonsaiOrHgChangesetId::Bonsai(cs_id)
    }
}

impl From<HgChangesetId> for BonsaiOrHgChangesetId {
    fn from(cs_id: HgChangesetId) -> Self {
        BonsaiOrHgChangesetId::Hg(cs_id)
    }
}

pub trait BonsaiHgMapping: Send + Sync {
    fn add(&self, entry: BonsaiHgMappingEntry) -> BoxFuture<bool, Error>;

    fn get(
        &self,
        repo_id: RepositoryId,
        cs_id: BonsaiOrHgChangesetId,
    ) -> BoxFuture<Option<BonsaiHgMappingEntry>, Error>;

    fn get_hg_from_bonsai(
        &self,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<HgChangesetId>, Error> {
        self.get(repo_id, cs_id.into())
            .map(|result| result.map(|entry| entry.hg_cs_id))
            .boxify()
    }

    fn get_bonsai_from_hg(
        &self,
        repo_id: RepositoryId,
        cs_id: HgChangesetId,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        self.get(repo_id, cs_id.into())
            .map(|result| result.map(|entry| entry.bcs_id))
            .boxify()
    }
}

impl BonsaiHgMapping for Arc<BonsaiHgMapping> {
    fn add(&self, entry: BonsaiHgMappingEntry) -> BoxFuture<bool, Error> {
        (**self).add(entry)
    }

    fn get(
        &self,
        repo_id: RepositoryId,
        cs_id: BonsaiOrHgChangesetId,
    ) -> BoxFuture<Option<BonsaiHgMappingEntry>, Error> {
        (**self).get(repo_id, cs_id)
    }
}

#[derive(Clone)]
pub struct SqlBonsaiHgMapping {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
}

queries! {
    write InsertMapping(values: (
        repo_id: RepositoryId,
        hg_cs_id: HgChangesetId,
        bcs_id: ChangesetId,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bonsai_hg_mapping (repo_id, hg_cs_id, bcs_id) VALUES {values}"
    }

    read SelectMappingByBonsai(repo_id: RepositoryId, bcs_id: ChangesetId) -> (HgChangesetId) {
        "SELECT hg_cs_id
         FROM bonsai_hg_mapping
         WHERE repo_id = {repo_id}
           AND bcs_id = {bcs_id}
         LIMIT 1"
    }

    read SelectMappingByHg(repo_id: RepositoryId, hg_cs_id: HgChangesetId) -> (ChangesetId) {
        "SELECT bcs_id
         FROM bonsai_hg_mapping
         WHERE repo_id = {repo_id}
           AND hg_cs_id = {hg_cs_id}
         LIMIT 1"
    }

    read SelectAnyMapping(
        repo_id: RepositoryId,
        hg_cs_id: HgChangesetId,
        bcs_id: ChangesetId,
    ) -> (HgChangesetId, ChangesetId) {
        "SELECT hg_cs_id, bcs_id
         FROM bonsai_hg_mapping
         WHERE repo_id = {repo_id}
           AND (hg_cs_id = {hg_cs_id} OR bcs_id = {bcs_id})
         LIMIT 1"
    }
}

impl SqlConstructors for SqlBonsaiHgMapping {
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
        include_str!("../schemas/sqlite-bonsai-hg-mapping.sql")
    }
}

impl BonsaiHgMapping for SqlBonsaiHgMapping {
    fn add(&self, entry: BonsaiHgMappingEntry) -> BoxFuture<bool, Error> {
        STATS::adds.add_value(1);
        cloned!(self.read_master_connection);

        let BonsaiHgMappingEntry {
            repo_id,
            hg_cs_id,
            bcs_id,
        } = entry.clone();

        InsertMapping::query(&self.write_connection, &[(&repo_id, &hg_cs_id, &bcs_id)])
            .and_then(move |result| {
                if result.affected_rows() == 1 {
                    Ok(true).into_future().left_future()
                } else {
                    SelectAnyMapping::query(&read_master_connection, &repo_id, &hg_cs_id, &bcs_id)
                        .and_then(move |rows| match rows.into_iter().next() {
                            Some(entry) if entry == (hg_cs_id, bcs_id) => Ok(false),
                            Some((hg_cs_id, bcs_id)) => Err(ErrorKind::ConflictingEntries(
                                BonsaiHgMappingEntry {
                                    repo_id,
                                    hg_cs_id,
                                    bcs_id,
                                },
                                entry,
                            ).into()),
                            None => Err(ErrorKind::RaceConditionWithDelete(entry).into()),
                        })
                        .right_future()
                }
            })
            .boxify()
    }

    fn get(
        &self,
        repo_id: RepositoryId,
        cs: BonsaiOrHgChangesetId,
    ) -> BoxFuture<Option<BonsaiHgMappingEntry>, Error> {
        STATS::gets.add_value(1);
        cloned!(self.read_master_connection);

        select_mapping(&self.read_connection, &repo_id, &cs)
            .and_then(move |maybe_mapping| match maybe_mapping {
                Some(mapping) => Ok(Some(mapping)).into_future().boxify(),
                None => {
                    STATS::gets_master.add_value(1);
                    select_mapping(&read_master_connection, &repo_id, &cs)
                }
            })
            .boxify()
    }
}

fn select_mapping(
    connection: &Connection,
    repo_id: &RepositoryId,
    cs_id: &BonsaiOrHgChangesetId,
) -> BoxFuture<Option<BonsaiHgMappingEntry>, Error> {
    cloned!(repo_id, cs_id);

    match cs_id {
        BonsaiOrHgChangesetId::Bonsai(bcs_id) => {
            SelectMappingByBonsai::query(&connection, &repo_id, &bcs_id)
                .map(move |rows| {
                    rows.into_iter()
                        .next()
                        .map(move |(hg_cs_id,)| BonsaiHgMappingEntry {
                            repo_id,
                            hg_cs_id,
                            bcs_id,
                        })
                })
                .boxify()
        }
        BonsaiOrHgChangesetId::Hg(hg_cs_id) => {
            SelectMappingByHg::query(&connection, &repo_id, &hg_cs_id)
                .map(move |rows| {
                    rows.into_iter()
                        .next()
                        .map(move |(bcs_id,)| BonsaiHgMappingEntry {
                            repo_id,
                            hg_cs_id,
                            bcs_id,
                        })
                })
                .boxify()
        }
    }
}
