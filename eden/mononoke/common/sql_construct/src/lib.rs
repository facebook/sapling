/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Common construction utilities for SQL database managers.
//!
//! Mononoke data stores that are backed by SQL databases are managed by a database manager, like
//! `SqlChangesets`, `SqlBookmarks`, etc.  This crate provides common utilities for constructing
//! these database managers, backed by various database types.
//!
//! Database managers should implement `SqlConstruct` to define how to be constructed from
//! a set of `SqlConnections`.  This is sufficient to allow construction based on `DatabaseConfig`,
//! which is provided through the `SqlConstructFromDatabaseConfig` trait.
//!
//! Database managers that support sharding should additionally implement `SqlShardedConstruct` for
//! the sharded case.
//!
//! Database managers that would like to be constructed from repository metadata configuration
//! should implement the `SqlConstructFromMetadataDatabaseConfig` trait.  If their data is not
//! stored in the primary metadata database, they should implement the `remote_database_config`
//! method to define which configuration is used for remote database configuration.
//!
//! Database managers that support sharding should instead implement the
//! `SqlShardableConstructFromMetadataDatabaseConfig` trait, which allows them to return
//! either sharded or unsharded configuration from `remote_database_config`.

mod config;
mod construct;
#[cfg(not(fbcode_build))]
mod oss;

pub use config::SqlConstructFromDatabaseConfig;
pub use config::SqlConstructFromMetadataDatabaseConfig;
pub use config::SqlShardableConstructFromMetadataDatabaseConfig;
pub use construct::SqlConstruct;
pub use construct::SqlShardedConstruct;

pub mod facebook {
    #[cfg(fbcode_build)]
    mod r#impl;

    #[cfg(fbcode_build)]
    pub use r#impl::FbSqlConstruct;
    #[cfg(fbcode_build)]
    pub use r#impl::FbSqlShardedConstruct;

    #[cfg(not(fbcode_build))]
    pub use crate::oss::FbSqlConstruct;
    #[cfg(not(fbcode_build))]
    pub use crate::oss::FbSqlShardedConstruct;
}
