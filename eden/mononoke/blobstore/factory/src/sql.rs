/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Error;
use cloned::cloned;
use fbinit::FacebookInit;
use metaconfig_types::LocalDatabaseConfig;
use metaconfig_types::MetadataDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use metaconfig_types::ShardableRemoteDatabaseConfig;
use sql::sqlite::SqliteMultithreaded;
use sql::sqlite::SqliteQueryType;
use sql::Connection;
use sql::SqlConnections;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_construct::SqlShardableConstructFromMetadataDatabaseConfig;
use sql_ext::facebook::MysqlOptions;
use sql_ext::open_existing_sqlite_path;
use sql_ext::open_sqlite_path;

use crate::ReadOnlyStorage;

/// Create instances of SQL database managers for repository metadata based on metadata database
/// config.
#[derive(Clone)]
pub struct MetadataSqlFactory {
    fb: FacebookInit,
    readonly: ReadOnlyStorage,
    connections_factory: MetadataSqlConnectionsFactory,
}

#[derive(Clone)]
pub enum MetadataSqlConnectionsFactory {
    Local {
        connections: SqlConnections,
        schema_connection: SqliteMultithreaded,
    },
    Remote {
        remote_config: RemoteMetadataDatabaseConfig,
        mysql_options: MysqlOptions,
    },
    OssRemote {
        remote_config: OssRemoteMetadataDatabaseConfig,
    },
}

#[derive(Clone)]
pub struct SqlTierInfo {
    pub tier_name: String,
    /// Returns None for unsharded, Some(number of shards) otherwise.
    /// NB does not tell you if shards are 0 or 1 based, just the overall number
    pub shard_num: Option<usize>,
}

impl MetadataSqlFactory {
    pub async fn new(
        fb: FacebookInit,
        dbconfig: MetadataDatabaseConfig,
        mysql_options: MysqlOptions,
        readonly: ReadOnlyStorage,
    ) -> Result<Self, Error> {
        let connections_factory = match dbconfig {
            MetadataDatabaseConfig::Local(LocalDatabaseConfig { path }) => {
                let path = path.join("sqlite_dbs");
                let schema_connection =
                    SqliteMultithreaded::new(open_sqlite_path(path.clone(), false)?);
                let read_connection =
                    Connection::with_sqlite(open_existing_sqlite_path(path, true)?);
                let connections = SqlConnections {
                    write_connection: if readonly.0 {
                        read_connection.clone()
                    } else {
                        schema_connection.clone().into()
                    },
                    read_master_connection: read_connection.clone(),
                    read_connection,
                };
                MetadataSqlConnectionsFactory::Local {
                    connections,
                    schema_connection,
                }
            }
            MetadataDatabaseConfig::Remote(remote_config) => {
                MetadataSqlConnectionsFactory::Remote {
                    remote_config,
                    mysql_options,
                }
            }
            MetadataDatabaseConfig::OssRemote(remote_config) => {
                MetadataSqlConnectionsFactory::OssRemote { remote_config }
            }
        };
        Ok(Self {
            fb,
            readonly,
            connections_factory,
        })
    }

    pub async fn open<T: SqlConstructFromMetadataDatabaseConfig>(&self) -> Result<T, Error> {
        match &self.connections_factory {
            MetadataSqlConnectionsFactory::Local {
                connections,
                schema_connection,
            } => {
                schema_connection
                    .acquire_sqlite_connection(SqliteQueryType::SchemaChange)
                    .await?
                    .execute_batch(T::CREATION_QUERY)?;
                Ok(T::from_sql_connections(connections.clone()))
            }
            MetadataSqlConnectionsFactory::Remote {
                remote_config,
                mysql_options,
            } => T::with_remote_metadata_database_config(
                self.fb,
                remote_config,
                mysql_options,
                self.readonly.0,
            ),
            MetadataSqlConnectionsFactory::OssRemote { remote_config } => {
                T::with_oss_remote_metadata_database_config(self.fb, remote_config, self.readonly.0)
                    .await
            }
        }
    }

    pub async fn open_shardable<T: SqlShardableConstructFromMetadataDatabaseConfig>(
        &self,
    ) -> Result<T, Error> {
        match &self.connections_factory {
            MetadataSqlConnectionsFactory::Local {
                connections,
                schema_connection,
            } => {
                schema_connection
                    .acquire_sqlite_connection(SqliteQueryType::SchemaChange)
                    .await?
                    .execute_batch(<T as SqlConstruct>::CREATION_QUERY)?;
                Ok(T::from_sql_connections(connections.clone()))
            }
            MetadataSqlConnectionsFactory::Remote {
                remote_config,
                mysql_options,
            } => {
                // Large numbers of shards means this can take some time.
                // Spawn it as a blocking task.
                tokio::task::spawn_blocking({
                    cloned!(self.fb, remote_config, mysql_options, self.readonly);
                    move || {
                        T::with_remote_metadata_database_config(
                            fb,
                            &remote_config,
                            &mysql_options,
                            readonly.0,
                        )
                    }
                })
                .await?
            }
            MetadataSqlConnectionsFactory::OssRemote { remote_config } => {
                T::with_oss_remote_metadata_database_config(self.fb, remote_config, self.readonly.0)
                    .await
            }
        }
    }

    pub fn tier_info_shardable<T: SqlShardableConstructFromMetadataDatabaseConfig>(
        &self,
    ) -> Result<SqlTierInfo, Error> {
        Ok(match &self.connections_factory {
            MetadataSqlConnectionsFactory::Local { .. } => SqlTierInfo {
                tier_name: "sqlite".to_string(),
                shard_num: None,
            },
            MetadataSqlConnectionsFactory::Remote { remote_config, .. } => {
                match T::remote_database_config(remote_config) {
                    Some(ShardableRemoteDatabaseConfig::Unsharded(config)) => SqlTierInfo {
                        tier_name: config.db_address.clone(),
                        shard_num: None,
                    },
                    Some(ShardableRemoteDatabaseConfig::Sharded(config)) => SqlTierInfo {
                        tier_name: config.shard_map.clone(),
                        shard_num: Some(config.shard_num.get()),
                    },
                    None => bail!(
                        "missing tier name in configuration for {}",
                        <T as SqlConstruct>::LABEL
                    ),
                }
            }
            MetadataSqlConnectionsFactory::OssRemote { .. } => SqlTierInfo {
                tier_name: "oss_mysql".to_string(),
                shard_num: None,
            },
        })
    }
}
