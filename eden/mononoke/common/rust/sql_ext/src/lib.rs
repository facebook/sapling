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

use sql::{Connection, Transaction};

pub use sqlite::{open_sqlite_in_memory, open_sqlite_path};

#[derive(Clone)]
pub struct SqlConnections {
    pub write_connection: Connection,
    pub read_connection: Connection,
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

#[must_use]
pub enum TransactionResult {
    Succeeded(Transaction),
    Failed,
}

pub mod facebook {
    #[cfg(fbcode_build)]
    mod r#impl;

    #[cfg(fbcode_build)]
    pub use r#impl::{
        create_myrouter_connections, create_raw_xdb_connections, myadmin, myrouter_ready,
    };

    #[cfg(not(fbcode_build))]
    pub use crate::oss::{create_myrouter_connections, create_raw_xdb_connections, myrouter_ready};

    #[derive(Copy, Clone, Debug)]
    pub struct MysqlOptions {
        pub myrouter_port: Option<u16>,
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

    pub struct PoolSizeConfig {
        pub write_pool_size: usize,
        pub read_pool_size: usize,
        pub read_master_pool_size: usize,
    }
}
