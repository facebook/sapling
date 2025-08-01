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
use rusqlite::Connection as SqliteConnection;
use sql::Connection as SqlConnection;
use sql::QueryTelemetry;
use sql::Transaction as SqlTransaction;
use sql::mysql;
use sql_common::sqlite::SqliteCallbacks;
pub use sql_query_telemetry::SqlQueryTelemetry;
pub use sqlite::open_existing_sqlite_path;
pub use sqlite::open_sqlite_in_memory;
pub use sqlite::open_sqlite_path;
use vec1::Vec1;

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
    pub use borrowed::borrowed;
    pub use clientinfo::ClientEntryPoint;
    pub use clientinfo::ClientRequestInfo;
    pub use mononoke_types::RepositoryId;
    pub use paste;
    pub use serde_json;
    pub use sql::WriteResult;
    pub use sql::queries;
    pub use sql_query_config::SqlQueryConfig;
    pub use sql_query_telemetry::SqlQueryTelemetry;
    pub use twox_hash::xxh3::Hash128;
    pub use twox_hash::xxh3::HasherExt;

    pub use crate::Connection;
    pub use crate::Transaction;
    pub use crate::mononoke_queries::CacheData;
    pub use crate::mononoke_queries::CachedQueryResult;
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

    pub sql_query_tel: SqlQueryTelemetry,

    pub shard_name: Option<String>,
}

impl Transaction {
    pub fn new(
        sql_txn: SqlTransaction,
        txn_telemetry: TransactionTelemetry,
        sql_query_tel: SqlQueryTelemetry,
        shard_name: Option<String>,
    ) -> Self {
        Self {
            inner: sql_txn,
            txn_telemetry,
            sql_query_tel,
            shard_name,
        }
    }

    pub fn add_sql_query_tel(self, sql_query_tel: SqlQueryTelemetry) -> Self {
        Self {
            sql_query_tel,
            ..self
        }
    }

    /// Perform a commit on this transaction
    pub async fn commit(self) -> Result<()> {
        let Transaction {
            inner: sql_txn,
            txn_telemetry,
            sql_query_tel,
            shard_name: _shard_name,
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
        sql_query_tel: SqlQueryTelemetry,
        query_repo_ids: Vec<RepositoryId>,
        granularity: TelemetryGranularity,
        query_name: &str,
        shard_name: Option<String>,
    ) -> Result<Self> {
        if let Some(tel) = opt_tel.as_ref() {
            txn_telemetry.add_query_telemetry(tel.clone())
        };

        txn_telemetry.add_repo_ids(query_repo_ids.clone());
        txn_telemetry.add_query_name(query_name);

        log_query_telemetry(
            opt_tel,
            &sql_query_tel,
            granularity,
            query_repo_ids,
            query_name,
        )?;

        Ok(Transaction::new(
            sql_txn,
            txn_telemetry,
            sql_query_tel,
            shard_name,
        ))
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

/// Wrapper over the SQL connection, needed for telemetry
#[derive(Clone, Debug)]
pub struct Connection {
    pub inner: SqlConnection,
    /// Name of the shard (i.e. DB) the connection belongs to
    pub shard_name: Option<String>,
}

impl From<Connection> for SqlConnection {
    fn from(conn: Connection) -> Self {
        conn.inner
    }
}

impl Connection {
    pub async fn start_transaction(&self, sql_query_tel: SqlQueryTelemetry) -> Result<Transaction> {
        let sql_txn = self.inner.start_transaction().await?;
        let txn_telemetry = Default::default();
        Ok(Transaction::new(
            sql_txn,
            txn_telemetry,
            sql_query_tel,
            self.shard_name.clone(),
        ))
    }

    pub fn sql_connection(&self) -> &SqlConnection {
        &self.inner
    }

    pub fn shard_name(&self) -> &Option<String> {
        &self.shard_name
    }

    pub fn with_sqlite(con: SqliteConnection) -> Self {
        let inner = SqlConnection::with_sqlite(con);
        Connection {
            inner,
            shard_name: None,
        }
    }

    /// Given a `rusqlite::Connection` create a connection to Sqlite database that might be used
    /// by this crate, and add callbacks for when operations happen.
    pub fn with_sqlite_callbacks(
        con: SqliteConnection,
        callbacks: Box<dyn SqliteCallbacks>,
    ) -> Self {
        let inner = SqlConnection::with_sqlite_callbacks(con, callbacks);
        Connection {
            inner,
            shard_name: None,
        }
    }
}

impl From<sql::sqlite::SqliteMultithreaded> for Connection {
    fn from(con: sql::sqlite::SqliteMultithreaded) -> Self {
        Connection {
            inner: SqlConnection::Sqlite(con),
            shard_name: None,
        }
    }
}

impl From<sql::mysql::OssConnection> for Connection {
    fn from(conn: mysql::OssConnection) -> Self {
        Connection {
            inner: SqlConnection::OssMysql(conn),
            shard_name: None,
        }
    }
}

/// Struct to store a set of write, read and read-only connections for a shard.
#[derive(Clone)]
pub struct SqlConnections {
    /// Write connection to the master
    pub write_connection: Connection,
    /// Read connection
    pub read_connection: Connection,
    /// Read master connection
    pub read_master_connection: Connection,
}

impl SqlConnections {
    /// Create SqlConnections from a single connection.
    pub fn new_single(connection: Connection) -> Self {
        Self {
            write_connection: connection.clone(),
            read_connection: connection.clone(),
            read_master_connection: connection,
        }
    }
}

impl From<SqlConnections> for sql::SqlConnections {
    fn from(conn: SqlConnections) -> Self {
        Self {
            write_connection: conn.write_connection.inner,
            read_connection: conn.read_connection.inner,
            read_master_connection: conn.read_master_connection.inner,
        }
    }
}

/// Struct to store a set of write, read and read-only connections for multiple shards.
#[derive(Clone)]
pub struct SqlShardedConnections {
    /// Write connections to the master for each shard
    pub write_connections: Vec1<Connection>,
    /// Read connections for each shard
    pub read_connections: Vec1<Connection>,
    /// Read master connections for each shard
    pub read_master_connections: Vec1<Connection>,
}

impl From<Vec1<SqlConnections>> for SqlShardedConnections {
    fn from(shard_connections: Vec1<SqlConnections>) -> Self {
        let (head, last) = shard_connections.split_off_last();
        let (write_connections, read_connections, read_master_connections) =
            itertools::multiunzip(head.into_iter().map(|conn| {
                (
                    conn.write_connection,
                    conn.read_connection,
                    conn.read_master_connection,
                )
            }));

        Self {
            read_connections: Vec1::from_vec_push(read_connections, last.read_connection),
            read_master_connections: Vec1::from_vec_push(
                read_master_connections,
                last.read_master_connection,
            ),
            write_connections: Vec1::from_vec_push(write_connections, last.write_connection),
        }
    }
}
