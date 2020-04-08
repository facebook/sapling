/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

/// We have a few jobs that maintain some counters for each Mononoke repository. For example,
/// the latest blobimported revision, latest replayed pushrebase etc. Previously these counters were
/// stored in Manifold, but that's not convenient. They are harder to modify and harder to keep
/// track of. Storing all of them in the same table makes maintenance easier and safer,
/// for example, we can have conditional updates.
use anyhow::Error;
use context::CoreContext;
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::RepositoryId;
use sql::{queries, Connection, Transaction as SqlTransaction};
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::{SqlConnections, TransactionResult};

pub trait MutableCounters: Send + Sync + 'static {
    /// Get the current value of the counter
    fn get_counter(
        &self,
        ctx: CoreContext,
        repoid: RepositoryId,
        name: &str,
    ) -> BoxFuture<Option<i64>, Error>;

    /// Set the current value of the counter. if `prev_value` is not None, then it sets the value
    /// conditionally.
    fn set_counter(
        &self,
        ctx: CoreContext,
        repoid: RepositoryId,
        name: &str,
        value: i64,
        prev_value: Option<i64>,
    ) -> BoxFuture<bool, Error>;
}

queries! {
    write SetCounter(
        repo_id: RepositoryId, name: &str, value: i64
    ) {
        none,
        mysql(
            "REPLACE INTO mutable_counters (repo_id, name, value) VALUES ({repo_id}, {name}, {value})"
        )
        sqlite(
            "REPLACE INTO mutable_counters (repo_id, name, value) VALUES ({repo_id}, CAST({name} AS TEXT), {value})"
        )
    }

    write SetCounterConditionally(
        repo_id: RepositoryId, name: &str, value: i64, prev_value: i64
    ) {
        none,
        mysql(
            "UPDATE mutable_counters SET value = {value}
            WHERE repo_id = {repo_id} AND name = {name} AND value = {prev_value}"
        )
        sqlite(
            "UPDATE mutable_counters SET value = {value}
            WHERE repo_id = {repo_id} AND name = CAST({name} AS TEXT) AND value = {prev_value}"
        )
    }

    read GetCounter(repo_id: RepositoryId, name: &str) -> (i64) {
        mysql(
            "SELECT value FROM mutable_counters WHERE repo_id = {repo_id} and name = {name}"
        )
        sqlite(
            "SELECT value FROM mutable_counters WHERE repo_id = {repo_id} and name = CAST({name} AS TEXT)"
        )
    }
}

#[derive(Clone)]
pub struct SqlMutableCounters {
    write_connection: Connection,
    read_master_connection: Connection,
}

impl SqlConstruct for SqlMutableCounters {
    const LABEL: &'static str = "mutable_counters";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-mutable-counters.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            write_connection: connections.write_connection,
            read_master_connection: connections.read_master_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlMutableCounters {}

impl MutableCounters for SqlMutableCounters {
    fn get_counter(
        &self,
        _ctx: CoreContext,
        repoid: RepositoryId,
        name: &str,
    ) -> BoxFuture<Option<i64>, Error> {
        GetCounter::query(&self.read_master_connection, &repoid, &name)
            .map(|counter| counter.first().map(|entry| entry.0))
            .boxify()
    }

    fn set_counter(
        &self,
        ctx: CoreContext,
        repoid: RepositoryId,
        name: &str,
        value: i64,
        prev_value: Option<i64>,
    ) -> BoxFuture<bool, Error> {
        self.write_connection
            .start_transaction()
            .and_then({
                let name = name.to_string();
                move |txn| Self::set_counter_on_txn(ctx, repoid, &name, value, prev_value, txn)
            })
            .and_then(|txn_result| match txn_result {
                TransactionResult::Succeeded(txn) => txn.commit().map(|()| true).left_future(),
                TransactionResult::Failed => future::ok(false).right_future(),
            })
            .boxify()
    }
}

impl SqlMutableCounters {
    pub fn set_counter_on_txn(
        _ctx: CoreContext,
        repoid: RepositoryId,
        name: &str,
        value: i64,
        prev_value: Option<i64>,
        txn: SqlTransaction,
    ) -> BoxFuture<TransactionResult, Error> {
        let f = match prev_value {
            Some(prev_value) => SetCounterConditionally::query_with_transaction(
                txn,
                &repoid,
                &name,
                &value,
                &prev_value,
            )
            .left_future(),
            None => SetCounter::query_with_transaction(txn, &repoid, &name, &value).right_future(),
        };

        f.map(|(txn, result)| {
            if result.affected_rows() >= 1 {
                TransactionResult::Succeeded(txn)
            } else {
                TransactionResult::Failed
            }
        })
        .boxify()
    }
}
