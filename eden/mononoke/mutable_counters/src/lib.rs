/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Mutable counters maintains numeric counters for each Mononoke repository.
//! These are used to maintain simple state about each repo, for example which
//! revisions have been blobimported, replayed, etc.
//!
//! The counter values themselves are stored in a table in the metadata
//! database.

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use context::PerfCounterType;
use mononoke_types::RepositoryId;
use sql::Transaction as SqlTransaction;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::mononoke_queries;
use sql_ext::SqlConnections;
use sql_ext::TransactionResult;
use stats::prelude::*;

define_stats! {
    prefix = "mononoke.mutable_counters";
    cur_value: dynamic_singleton_counter("{}.cur_value", (name: String)),
}

#[facet::facet]
#[async_trait]
pub trait MutableCounters {
    /// Get the current value of the counter
    async fn get_counter(&self, ctx: &CoreContext, name: &str) -> Result<Option<i64>>;

    async fn get_maybe_stale_counter(&self, ctx: &CoreContext, name: &str) -> Result<Option<i64>>;

    /// Set the current value of the counter. if `prev_value` is not None,
    /// then the value is only updated if the previous value matches.
    async fn set_counter(
        &self,
        ctx: &CoreContext,
        name: &str,
        value: i64,
        prev_value: Option<i64>,
    ) -> Result<bool>;

    /// Get the names and values of all the counters for the repository.
    async fn get_all_counters(&self, ctx: &CoreContext) -> Result<Vec<(String, i64)>>;
}

mononoke_queries! {
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

    read GetCountersForRepo(repo_id: RepositoryId) -> (String, i64) {
        "SELECT name, value FROM mutable_counters WHERE repo_id = {repo_id} ORDER BY name"
    }
}

pub struct SqlMutableCounters {
    repo_id: RepositoryId,
    connections: SqlConnections,
}

pub struct SqlMutableCountersBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlMutableCountersBuilder {
    const LABEL: &'static str = "mutable_counters";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-mutable-counters.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlMutableCountersBuilder {}

impl SqlMutableCountersBuilder {
    pub fn build(self, repo_id: RepositoryId) -> SqlMutableCounters {
        SqlMutableCounters {
            repo_id,
            connections: self.connections,
        }
    }
}

#[async_trait]
impl MutableCounters for SqlMutableCounters {
    async fn get_counter(&self, ctx: &CoreContext, name: &str) -> Result<Option<i64>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);
        let conn = &self.connections.read_master_connection;
        let counter = GetCounter::query(conn, &self.repo_id, &name).await?;
        Ok(counter.first().map(|entry| entry.0))
    }

    async fn get_maybe_stale_counter(&self, ctx: &CoreContext, name: &str) -> Result<Option<i64>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let conn = &self.connections.read_connection;
        let counter = GetCounter::query(conn, &self.repo_id, &name).await?;
        Ok(counter.first().map(|entry| entry.0))
    }

    async fn set_counter(
        &self,
        ctx: &CoreContext,
        name: &str,
        value: i64,
        prev_value: Option<i64>,
    ) -> Result<bool> {
        let conn = &self.connections.write_connection;
        let txn = conn.start_transaction().await?;
        let txn_result =
            Self::set_counter_on_txn(ctx, self.repo_id, name, value, prev_value, txn).await?;
        match txn_result {
            TransactionResult::Succeeded(txn) => {
                txn.commit().await?;
                STATS::cur_value.set_value(ctx.fb, value, (name.to_owned(),));
                Ok(true)
            }
            TransactionResult::Failed => Ok(false),
        }
    }

    async fn get_all_counters(&self, ctx: &CoreContext) -> Result<Vec<(String, i64)>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);
        let conn = &self.connections.read_master_connection;
        let counters = GetCountersForRepo::query(conn, &self.repo_id).await?;
        Ok(counters.into_iter().collect())
    }
}

impl SqlMutableCounters {
    pub async fn set_counter_on_txn(
        ctx: &CoreContext,
        repo_id: RepositoryId,
        name: &str,
        value: i64,
        prev_value: Option<i64>,
        txn: SqlTransaction,
    ) -> Result<TransactionResult> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        let (txn, result) = if let Some(prev_value) = prev_value {
            SetCounterConditionally::query_with_transaction(
                txn,
                &repo_id,
                &name,
                &value,
                &prev_value,
            )
            .await?
        } else {
            SetCounter::query_with_transaction(txn, &repo_id, &name, &value).await?
        };

        Ok(if result.affected_rows() >= 1 {
            TransactionResult::Succeeded(txn)
        } else {
            TransactionResult::Failed
        })
    }
}
