/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Error};
use fbinit::FacebookInit;
use metaconfig_types::{
    LocalDatabaseConfig, MetadataDatabaseConfig, ShardableRemoteDatabaseConfig,
};
use sql::Connection;
use sql_construct::{
    SqlConstructFromMetadataDatabaseConfig, SqlShardableConstructFromMetadataDatabaseConfig,
};
use sql_ext::{
    facebook::{create_mysql_connections_unsharded, MysqlOptions},
    open_sqlite_path, SqlConnections,
};

use crate::ReadOnlyStorage;

/// Create instances of SQL database managers for repository metadata based on metadata database
/// config.
#[derive(Clone)]
pub struct MetadataSqlFactory {
    fb: FacebookInit,
    dbconfig: MetadataDatabaseConfig,
    mysql_options: MysqlOptions,
    readonly: ReadOnlyStorage,
}

#[derive(Clone)]
pub struct SqlTierInfo {
    pub tier_name: String,
    /// Returns None for unsharded, Some(number of shards) otherwise.
    /// NB does not tell you if shards are 0 or 1 based, just the overall number
    pub shard_num: Option<usize>,
}

impl MetadataSqlFactory {
    pub fn open<T: SqlConstructFromMetadataDatabaseConfig>(&self) -> Result<T, Error> {
        T::with_metadata_database_config(
            self.fb,
            &self.dbconfig,
            &self.mysql_options,
            self.readonly.0,
        )
    }

    pub fn open_shardable<T: SqlShardableConstructFromMetadataDatabaseConfig>(
        &self,
    ) -> Result<T, Error> {
        T::with_metadata_database_config(
            self.fb,
            &self.dbconfig,
            &self.mysql_options,
            self.readonly.0,
        )
    }

    pub fn tier_info_shardable<T: SqlShardableConstructFromMetadataDatabaseConfig>(
        &self,
    ) -> Result<SqlTierInfo, Error> {
        Ok(match &self.dbconfig {
            MetadataDatabaseConfig::Local(_) => SqlTierInfo {
                tier_name: "sqlite".to_string(),
                shard_num: None,
            },
            MetadataDatabaseConfig::Remote(remote) => match T::remote_database_config(remote) {
                Some(ShardableRemoteDatabaseConfig::Unsharded(config)) => SqlTierInfo {
                    tier_name: config.db_address.clone(),
                    shard_num: None,
                },
                Some(ShardableRemoteDatabaseConfig::Sharded(config)) => SqlTierInfo {
                    tier_name: config.shard_map.clone(),
                    shard_num: Some(config.shard_num.get()),
                },
                None => bail!("missing tier name in configuration"),
            },
        })
    }

    /// Make connections to the primary metadata database
    pub async fn make_primary_connections(&self, label: String) -> Result<SqlConnections, Error> {
        match &self.dbconfig {
            MetadataDatabaseConfig::Local(LocalDatabaseConfig { path }) => {
                open_sqlite_path(path.join("sqlite_dbs"), self.readonly.0)
                    .map(|conn| SqlConnections::new_single(Connection::with_sqlite(conn)))
            }
            MetadataDatabaseConfig::Remote(config) => create_mysql_connections_unsharded(
                self.fb,
                self.mysql_options.clone(),
                label,
                config.primary.db_address.clone(),
                self.readonly.0,
            ),
        }
    }
}

pub async fn make_metadata_sql_factory(
    fb: FacebookInit,
    dbconfig: MetadataDatabaseConfig,
    mysql_options: MysqlOptions,
    readonly: ReadOnlyStorage,
) -> Result<MetadataSqlFactory, Error> {
    Ok(MetadataSqlFactory {
        fb,
        dbconfig,
        mysql_options,
        readonly,
    })
}
