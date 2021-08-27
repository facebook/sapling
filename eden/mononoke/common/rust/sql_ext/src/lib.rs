/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(not(fbcode_build))]
mod oss;
pub mod replication;
mod sqlite;

use sql::Transaction;

pub use sql::{SqlConnections, SqlShardedConnections};
pub use sqlite::{open_existing_sqlite_path, open_sqlite_in_memory, open_sqlite_path};

#[must_use]
pub enum TransactionResult {
    Succeeded(Transaction),
    Failed,
}

pub mod facebook {
    #[cfg(fbcode_build)]
    mod r#impl;

    use std::fmt::{self, Debug};

    #[cfg(fbcode_build)]
    pub use r#impl::{
        create_mysql_connections_sharded, create_mysql_connections_unsharded,
        myadmin::{MyAdmin, MyAdminLagMonitor},
        PoolConfig, SharedConnectionPool,
    };

    #[cfg(not(fbcode_build))]
    pub use crate::oss::{
        create_mysql_connections_sharded, create_mysql_connections_unsharded, MyAdmin,
        MyAdminLagMonitor, PoolConfig, SharedConnectionPool,
    };

    /// MySQL global shared connection pool configuration.
    #[derive(Clone)]
    pub struct MysqlOptions {
        pub pool: SharedConnectionPool,
        // pool config is used only once when the shared connection pool is being created
        pub pool_config: PoolConfig,
        pub master_only: bool,
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

        pub fn read_connection_type(&self) -> ReadConnectionType {
            if self.master_only {
                ReadConnectionType::Master
            } else {
                ReadConnectionType::Replica
            }
        }
    }

    impl Debug for MysqlOptions {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let conn_type = if self.master_only {
                "master only"
            } else {
                "replica"
            };
            write!(
                f,
                "MySQL pool with config {:?}, connection type: {}",
                self.pool_config, conn_type
            )
        }
    }

    #[derive(Copy, Clone, Debug)]
    pub enum ReadConnectionType {
        Replica,
        Master,
    }
}
