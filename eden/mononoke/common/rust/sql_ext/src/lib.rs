/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod constructors;
mod sqlite;

use sql::Transaction;

pub use constructors::{SqlConnections, SqlConstructors};
pub use sqlite::{create_sqlite_connections, open_sqlite_in_memory, open_sqlite_path};

#[must_use]
pub enum TransactionResult {
    Succeeded(Transaction),
    Failed,
}

pub mod facebook {
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

    pub use r#impl::*;

    #[cfg(fbcode_build)]
    mod r#impl;

    #[cfg(not(fbcode_build))]
    mod r#impl {
        use crate::{facebook::*, *};

        use anyhow::Error;
        use fbinit::FacebookInit;
        use futures_ext::BoxFuture;
        use metaconfig_types::MetadataDBConfig;
        use slog::Logger;

        macro_rules! fb_unimplemented {
            () => {
                unimplemented!("This is implemented only for fbcode_build!")
            };
        }

        impl PoolSizeConfig {
            pub fn for_regular_connection() -> Self {
                fb_unimplemented!()
            }

            pub fn for_sharded_connection() -> Self {
                fb_unimplemented!()
            }
        }

        pub fn create_myrouter_connections(
            _: String,
            _: Option<usize>,
            _: u16,
            _: ReadConnectionType,
            _: PoolSizeConfig,
            _: String,
            _: bool,
        ) -> SqlConnections {
            fb_unimplemented!()
        }

        pub fn myrouter_ready(
            _: Option<String>,
            _: MysqlOptions,
            _: Logger,
        ) -> BoxFuture<(), Error> {
            fb_unimplemented!()
        }

        pub fn create_raw_xdb_connections(
            _: FacebookInit,
            _: String,
            _: ReadConnectionType,
            _: bool,
        ) -> BoxFuture<SqlConnections, Error> {
            fb_unimplemented!()
        }

        impl<T: SqlConstructors> FbSqlConstructors for T {}

        /// Set of useful constructors for Mononoke's sql based data access objects
        pub trait FbSqlConstructors: SqlConstructors + Sized + Send + Sync + 'static {
            fn with_myrouter(_: String, _: u16, _: ReadConnectionType, _: bool) -> Self {
                fb_unimplemented!()
            }

            fn with_raw_xdb_tier(
                _: FacebookInit,
                _: String,
                _: ReadConnectionType,
                _: bool,
            ) -> BoxFuture<Self, Error> {
                fb_unimplemented!()
            }

            fn with_xdb(
                _: FacebookInit,
                _: String,
                _: MysqlOptions,
                _: bool,
            ) -> BoxFuture<Self, Error> {
                fb_unimplemented!()
            }

            fn with_db_config(
                _: FacebookInit,
                _: &MetadataDBConfig,
                _: MysqlOptions,
                _: bool,
            ) -> BoxFuture<Self, Error> {
                fb_unimplemented!()
            }
        }
    }
}
