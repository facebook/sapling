/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Result;
use async_trait::async_trait;
use mononoke_types::{MononokeId, RawBundle2Id};
use sql::{queries, Connection};
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::SqlConnections;
use stats::prelude::*;

define_stats! {
    prefix = "mononoke.reversefillerqueue";
    insert_bundle: dynamic_timeseries("{}.insert_bundle", (reponame: String); Sum),
}

queries! {
    write InsertBundle(values: (reponame: String, bundle: String)) {
        none,
        "INSERT INTO reversefillerqueue (reponame, bundle) VALUES {values}"
    }
}

#[async_trait]
pub trait ReverseFillerQueue: Send + Sync + 'static {
    #[allow(clippy::ptr_arg)]
    async fn insert_bundle(&self, reponame: &String, raw_bundle2_id: &RawBundle2Id) -> Result<()>;
}

#[derive(Clone, Debug)]
pub struct SqlReverseFillerQueue {
    write_connection: Connection,
}

impl SqlConstruct for SqlReverseFillerQueue {
    const LABEL: &'static str = "reversefillerqueue";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-reversefillerqueue.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            write_connection: connections.write_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlReverseFillerQueue {}

#[async_trait]
impl ReverseFillerQueue for SqlReverseFillerQueue {
    #[allow(clippy::ptr_arg)]
    async fn insert_bundle(&self, reponame: &String, raw_bundle2_id: &RawBundle2Id) -> Result<()> {
        let raw_bundle2_id = raw_bundle2_id.blobstore_key();
        STATS::insert_bundle.add_value(1, (reponame.to_owned(),));
        InsertBundle::query(&self.write_connection, &[(reponame, &raw_bundle2_id)]).await?;
        Ok(())
    }
}
