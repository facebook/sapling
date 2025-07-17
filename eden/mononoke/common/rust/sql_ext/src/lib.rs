/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod mononoke_queries;
#[cfg(not(fbcode_build))]
mod oss;
pub mod replication;
mod sqlite;
mod telemetry;
#[cfg(test)]
mod tests;

use anyhow::Result;
use mononoke_types::RepositoryId;
use sql::Connection;
use sql::QueryTelemetry;
pub use sql::SqlConnections;
pub use sql::SqlShardedConnections;
use sql::Transaction as SqlTransaction;
pub use sql_query_telemetry::SqlQueryTelemetry;
pub use sqlite::open_existing_sqlite_path;
pub use sqlite::open_sqlite_in_memory;
pub use sqlite::open_sqlite_path;

pub use crate::mononoke_queries::should_retry_mysql_query as should_retry_query;
use crate::telemetry::TelemetryGranularity;
use crate::telemetry::TransactionTelemetry;
use crate::telemetry::log_query_telemetry;
use crate::telemetry::log_transaction_telemetry;

#[must_use]
pub enum TransactionResult {
    Succeeded(Transaction),
    Failed,
}

pub mod _macro_internal {
    pub use std::collections::hash_map::DefaultHasher;
    pub use std::hash::Hash;
    pub use std::hash::Hasher;
    pub use std::sync::Arc;

    pub use anyhow::Result;
    pub use clientinfo::ClientEntryPoint;
    pub use clientinfo::ClientRequestInfo;
    pub use mononoke_types::RepositoryId;
    pub use paste;
    pub use serde_json;
    pub use sql::Connection;
    pub use sql::WriteResult;
    pub use sql::queries;
    pub use sql_query_config::SqlQueryConfig;
    pub use sql_query_telemetry::SqlQueryTelemetry;
    pub use twox_hash::xxh3::Hash128;
    pub use twox_hash::xxh3::HasherExt;

    pub use crate::Transaction;
    pub use crate::mononoke_queries::CacheData;
    pub use crate::mononoke_queries::CachedQueryResult;
    pub use crate::mononoke_queries::build_transaction_wrapper;
    pub use crate::mononoke_queries::query_with_retry;
    pub use crate::mononoke_queries::query_with_retry_no_cache;
    pub use crate::telemetry::TelemetryGranularity;
    pub use crate::telemetry::log_query_error;
    pub use crate::telemetry::log_query_telemetry;
    pub use crate::telemetry::log_transaction_telemetry;
}

/// Wrapper over the SQL transaction that will keep track of telemetry from the
/// entire transaction.
pub struct Transaction {
    pub inner: SqlTransaction,

    pub txn_telemetry: TransactionTelemetry,

    // TODO(T223577767): make this required after updating all callsites
    pub sql_query_tel: Option<SqlQueryTelemetry>,
}

impl Transaction {
    pub fn new(
        sql_txn: SqlTransaction,
        txn_telemetry: TransactionTelemetry,
        sql_query_tel: Option<SqlQueryTelemetry>,
    ) -> Self {
        Self {
            inner: sql_txn,
            txn_telemetry,
            sql_query_tel,
        }
    }

    pub fn add_sql_query_tel(self, sql_query_tel: SqlQueryTelemetry) -> Self {
        Self {
            sql_query_tel: Some(sql_query_tel),
            ..self
        }
    }

    /// Create a new transaction for the provided connection.
    pub async fn from_connection(connection: &Connection) -> Result<Self> {
        let inner = SqlTransaction::new(connection).await?;

        Ok(Self {
            inner,
            txn_telemetry: Default::default(),
            sql_query_tel: None,
        })
    }

    /// Create a new transaction for the provided connection.
    pub fn from_sql_transaction(sql_txn: SqlTransaction) -> Self {
        Self {
            inner: sql_txn,
            txn_telemetry: Default::default(),
            sql_query_tel: None,
        }
    }

    /// Perform a commit on this transaction
    pub async fn commit(self) -> Result<()> {
        let Transaction {
            inner: sql_txn,
            txn_telemetry,
            sql_query_tel,
        } = self;

        log_transaction_telemetry(txn_telemetry, sql_query_tel)?;

        sql_txn.commit().await
    }

    /// Perform a rollback on this transaction
    pub async fn rollback(self) -> Result<()> {
        self.inner.rollback().await
    }

    pub fn from_transaction_query_result(
        sql_txn: SqlTransaction,
        opt_tel: Option<QueryTelemetry>,
        mut txn_telemetry: TransactionTelemetry,
        tel_logger: Option<SqlQueryTelemetry>,
        query_repo_ids: Vec<RepositoryId>,
        granularity: TelemetryGranularity,
        query_name: &str,
    ) -> Result<Self> {
        if let Some(tel) = opt_tel.as_ref() {
            txn_telemetry.add_query_telemetry(tel.clone())
        };

        txn_telemetry.add_repo_ids(query_repo_ids.clone());

        log_query_telemetry(
            opt_tel,
            tel_logger.as_ref(),
            granularity,
            query_repo_ids,
            query_name,
        )?;

        Ok(Transaction::new(sql_txn, txn_telemetry, tel_logger))
    }
}

impl From<SqlTransaction> for Transaction {
    fn from(sql_txn: SqlTransaction) -> Self {
        Self {
            inner: sql_txn,
            txn_telemetry: Default::default(),
            sql_query_tel: None,
        }
    }
}

pub mod facebook {
    #[cfg(fbcode_build)]
    mod r#impl;

    use std::fmt;
    use std::fmt::Debug;

    #[cfg(fbcode_build)]
    pub use r#impl::PoolConfig;
    #[cfg(fbcode_build)]
    pub use r#impl::SharedConnectionPool;
    #[cfg(fbcode_build)]
    pub use r#impl::create_mysql_connections_sharded;
    #[cfg(fbcode_build)]
    pub use r#impl::create_mysql_connections_unsharded;
    #[cfg(fbcode_build)]
    pub use r#impl::create_oss_mysql_connections_unsharded;
    #[cfg(fbcode_build)]
    pub use r#impl::myadmin::MyAdmin;
    #[cfg(fbcode_build)]
    pub use r#impl::myadmin::MyAdminLagMonitor;
    #[cfg(fbcode_build)]
    pub use r#impl::myadmin::replication_status_chunked;

    #[cfg(not(fbcode_build))]
    pub use crate::oss::MyAdmin;
    #[cfg(not(fbcode_build))]
    pub use crate::oss::MyAdminLagMonitor;
    #[cfg(not(fbcode_build))]
    pub use crate::oss::PoolConfig;
    #[cfg(not(fbcode_build))]
    pub use crate::oss::SharedConnectionPool;
    #[cfg(not(fbcode_build))]
    pub use crate::oss::create_mysql_connections_sharded;
    #[cfg(not(fbcode_build))]
    pub use crate::oss::create_mysql_connections_unsharded;

    /// MySQL global shared connection pool configuration.
    #[derive(Clone, Default)]
    pub struct MysqlOptions {
        pub pool: SharedConnectionPool,
        // pool config is used only once when the shared connection pool is being created
        pub pool_config: PoolConfig,
        pub read_connection_type: ReadConnectionType,
    }

    impl MysqlOptions {
        pub fn per_key_limit(&self) -> Option<usize> {
            #[cfg(not(fbcode_build))]
            {
                None
            }
            #[cfg(fbcode_build)]
            {
                Some(self.pool_config.per_key_limit as usize)
            }
        }
    }

    impl Debug for MysqlOptions {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(
                f,
                "MySQL pool with config {:?}, connection type: {:?}",
                self.pool_config, self.read_connection_type
            )
        }
    }

    /// Mirrors facebook::db::InstanceRequirement enum for DBLocator
    #[derive(Copy, Clone, Debug, Default)]
    pub enum ReadConnectionType {
        /// Choose master or replica, whatever is closest and available.
        /// Use this if both master and replica are in the same region, and reads
        /// should we served by both.
        Closest,
        /// Choose replicas only, avoiding the master, even if it means going to a
        /// remote region.
        #[default]
        ReplicaOnly,
        /// Choose master only (typically for writes). Will never connect to replica.
        Master,
        /// Choose closer first and inside the same region, replicas first.
        /// In case both master and replica in the same region - all reads
        /// will be routed to the replica.
        ReplicaFirst,
        /// Choose replicas that satisfy a lower bound HLC value in order to
        /// perform consistent read-your-writes operations
        ReadAfterWriteConsistency,
    }
}
