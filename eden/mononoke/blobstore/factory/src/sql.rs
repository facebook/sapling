/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Error;
use fbinit::FacebookInit;
use metaconfig_types::LocalDatabaseConfig;
use metaconfig_types::MetadataDatabaseConfig;
use metaconfig_types::ShardableRemoteDatabaseConfig;
use sql::Connection;
use sql::SqlConnections;
use sql::SqlConnectionsWithSchema;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_construct::SqlShardableConstructFromMetadataDatabaseConfig;
use sql_ext::facebook::create_mysql_connections_unsharded;
use sql_ext::facebook::MysqlOptions;
use sql_ext::open_existing_sqlite_path;
use sql_ext::open_sqlite_path;

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
    pub async fn make_primary_connections(
        &self,
        label: String,
    ) -> Result<SqlConnectionsWithSchema, Error> {
        match &self.dbconfig {
            MetadataDatabaseConfig::Local(LocalDatabaseConfig { path }) => {
                let path = path.join("sqlite_dbs");
                let schema_connection =
                    Connection::with_sqlite(open_sqlite_path(path.clone(), false)?);
                let read_connection =
                    Connection::with_sqlite(open_existing_sqlite_path(path, true)?);
                Ok(SqlConnectionsWithSchema::new(
                    SqlConnections {
                        write_connection: if self.readonly.0 {
                            read_connection.clone()
                        } else {
                            schema_connection.clone()
                        },
                        read_master_connection: read_connection.clone(),
                        read_connection,
                    },
                    Some(schema_connection),
                ))
            }
            MetadataDatabaseConfig::Remote(config) => Ok(SqlConnectionsWithSchema::new(
                create_mysql_connections_unsharded(
                    self.fb,
                    self.mysql_options.clone(),
                    label,
                    config.primary.db_address.clone(),
                    self.readonly.0,
                )?,
                None,
            )),
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
