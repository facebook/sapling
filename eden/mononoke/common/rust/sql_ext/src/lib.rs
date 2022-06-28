/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(not(fbcode_build))]
mod oss;
pub mod replication;
mod sqlite;

use sql::Transaction;

pub use sql::SqlConnections;
pub use sql::SqlShardedConnections;
pub use sqlite::open_existing_sqlite_path;
pub use sqlite::open_sqlite_in_memory;
pub use sqlite::open_sqlite_path;

#[must_use]
pub enum TransactionResult {
    Succeeded(Transaction),
    Failed,
}

pub mod facebook {
    #[cfg(fbcode_build)]
    mod r#impl;

    use std::fmt;
    use std::fmt::Debug;

    #[cfg(fbcode_build)]
    pub use r#impl::create_mysql_connections_sharded;
    #[cfg(fbcode_build)]
    pub use r#impl::create_mysql_connections_unsharded;
    #[cfg(fbcode_build)]
    pub use r#impl::myadmin::replication_status_chunked;
    #[cfg(fbcode_build)]
    pub use r#impl::myadmin::MyAdmin;
    #[cfg(fbcode_build)]
    pub use r#impl::myadmin::MyAdminLagMonitor;
    #[cfg(fbcode_build)]
    pub use r#impl::PoolConfig;
    #[cfg(fbcode_build)]
    pub use r#impl::SharedConnectionPool;

    #[cfg(not(fbcode_build))]
    pub use crate::oss::create_mysql_connections_sharded;
    #[cfg(not(fbcode_build))]
    pub use crate::oss::create_mysql_connections_unsharded;
    #[cfg(not(fbcode_build))]
    pub use crate::oss::MyAdmin;
    #[cfg(not(fbcode_build))]
    pub use crate::oss::MyAdminLagMonitor;
    #[cfg(not(fbcode_build))]
    pub use crate::oss::PoolConfig;
    #[cfg(not(fbcode_build))]
    pub use crate::oss::SharedConnectionPool;

    /// MySQL global shared connection pool configuration.
    #[derive(Clone)]
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
    #[derive(Copy, Clone, Debug)]
    pub enum ReadConnectionType {
        /// Choose master or replica, whatever is closest and available.
        /// Use this if both master and replica are in the same region, and reads
        /// should we served by both.
        Closest,
        /// Choose replicas only, avoiding the master, even if it means going to a
        /// remote region.
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
