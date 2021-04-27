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
        create_myrouter_connections, create_mysql_connections_sharded,
        create_mysql_connections_unsharded, deprecated_create_mysql_pool_unsharded,
        myadmin::{MyAdmin, MyAdminLagMonitor},
        myrouter_ready, PoolConfig, SharedConnectionPool,
    };

    #[cfg(not(fbcode_build))]
    pub use crate::oss::{
        create_myrouter_connections, create_mysql_connections_sharded,
        create_mysql_connections_unsharded, deprecated_create_mysql_pool_unsharded, myrouter_ready,
        MyAdmin, MyAdminLagMonitor, PoolConfig, SharedConnectionPool,
    };

    /// Way to connect to the DB: via myrouter connections, raw xdb or Mysql client
    #[derive(Clone)]
    pub enum MysqlConnectionType {
        Myrouter(u16),
        Mysql(SharedConnectionPool, PoolConfig),
    }

    impl MysqlConnectionType {
        pub fn per_key_limit(&self) -> Option<usize> {
            #[cfg(not(fbcode_build))]
            {
                None
            }
            #[cfg(fbcode_build)]
            match self {
                Self::Myrouter(_) => None,
                Self::Mysql(_, pool_config) => Some(pool_config.per_key_limit as usize),
            }
        }
    }

    impl Debug for MysqlConnectionType {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match &self {
                Self::Myrouter(port) => write!(f, "MyRouter(port: {:?})", port),
                Self::Mysql(_, config) => write!(f, "MySQL with config {:?}", config),
            }
        }
    }

    #[derive(Debug, Clone)]
    pub struct MysqlOptions {
        pub connection_type: MysqlConnectionType,
        pub master_only: bool,
    }

    impl MysqlOptions {
        pub fn read_connection_type(&self) -> ReadConnectionType {
            if self.master_only {
                ReadConnectionType::Master
            } else {
                ReadConnectionType::Replica
            }
        }
    }

    #[derive(Copy, Clone, Debug)]
    pub enum ReadConnectionType {
        Replica,
        Master,
    }

    #[derive(Copy, Clone, Debug)]
    pub struct PoolSizeConfig {
        pub write_pool_size: usize,
        pub read_pool_size: usize,
        pub read_master_pool_size: usize,
    }
}
