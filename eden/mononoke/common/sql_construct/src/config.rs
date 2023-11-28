/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use fbinit::FacebookInit;
use metaconfig_types::DatabaseConfig;
use metaconfig_types::LocalDatabaseConfig;
use metaconfig_types::MetadataDatabaseConfig;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use metaconfig_types::ShardableRemoteDatabaseConfig;
use metaconfig_types::ShardedDatabaseConfig;
use sql_ext::facebook::MysqlOptions;

use crate::construct::SqlConstruct;
use crate::facebook::FbSqlConstruct;
use crate::facebook::FbSqlShardedConstruct;

/// Trait that allows construction from database config.
pub trait SqlConstructFromDatabaseConfig: FbSqlConstruct + SqlConstruct {
    fn with_database_config(
        fb: FacebookInit,
        database_config: &DatabaseConfig,
        mysql_options: &MysqlOptions,
        readonly: bool,
    ) -> Result<Self> {
        match database_config {
            DatabaseConfig::Local(LocalDatabaseConfig { path }) => {
                Self::with_sqlite_path(path.join("sqlite_dbs"), readonly)
            }
            DatabaseConfig::Remote(config) => {
                Self::with_mysql(fb, config.db_address.clone(), mysql_options, readonly)
            }
        }
        .with_context(|| {
            format!(
                "While connecting to {:?} (with options {:?})",
                database_config, mysql_options
            )
        })
    }
}

#[async_trait::async_trait]
impl<T: SqlConstruct + FbSqlConstruct> SqlConstructFromDatabaseConfig for T {}

/// Trait that allows construction from sharded database config.
pub trait SqlConstructFromShardedDatabaseConfig: FbSqlShardedConstruct {
    fn with_sharded_database_config(
        fb: FacebookInit,
        database_config: &ShardedDatabaseConfig,
        mysql_options: &MysqlOptions,
        readonly: bool,
    ) -> Result<Self> {
        match database_config {
            ShardedDatabaseConfig::Local(LocalDatabaseConfig { path }) => {
                Self::with_sqlite_path(path.join("sqlite_dbs"), readonly)
            }
            ShardedDatabaseConfig::Sharded(config) => Self::with_sharded_mysql(
                fb,
                config.shard_map.clone(),
                config.shard_num,
                mysql_options,
                readonly,
            ),
            ShardedDatabaseConfig::Unsharded(config) => {
                Self::with_mysql(fb, config.db_address.clone(), mysql_options, readonly)
            }
        }
        .with_context(|| {
            format!(
                "While connecting to {:?} (with options {:?})",
                database_config, mysql_options
            )
        })
    }
}

#[async_trait::async_trait]
impl<T: FbSqlShardedConstruct> SqlConstructFromShardedDatabaseConfig for T {}

/// Trait that allows construction from the metadata database config.
#[async_trait::async_trait]
pub trait SqlConstructFromMetadataDatabaseConfig: FbSqlConstruct + SqlConstruct {
    async fn with_metadata_database_config(
        fb: FacebookInit,
        metadata_database_config: &MetadataDatabaseConfig,
        mysql_options: &MysqlOptions,
        readonly: bool,
    ) -> Result<Self> {
        match metadata_database_config {
            MetadataDatabaseConfig::Local(LocalDatabaseConfig { path }) => {
                Self::with_sqlite_path(path.join("sqlite_dbs"), readonly)
            }
            MetadataDatabaseConfig::Remote(remote) => {
                Self::with_remote_metadata_database_config(fb, remote, mysql_options, readonly)
            }
            MetadataDatabaseConfig::OssRemote(ossremote) => {
                Self::with_oss_remote_metadata_database_config(fb, ossremote, readonly).await
            }
        }
    }

    fn with_remote_metadata_database_config(
        fb: FacebookInit,
        remote: &RemoteMetadataDatabaseConfig,
        mysql_options: &MysqlOptions,
        readonly: bool,
    ) -> Result<Self> {
        let config = Self::remote_database_config(remote)
            .ok_or_else(|| anyhow!("no configuration available"))?;
        Self::with_mysql(fb, config.db_address.clone(), mysql_options, readonly)
    }

    async fn with_oss_remote_metadata_database_config(
        fb: FacebookInit,
        remote: &OssRemoteMetadataDatabaseConfig,
        readonly: bool,
    ) -> Result<Self> {
        let config = Self::oss_remote_database_config(remote)
            .ok_or_else(|| anyhow!("no configuration available"))?;

        Self::with_oss_mysql(
            fb,
            config.host.clone(),
            config.port,
            config.database.clone(),
            config.secret_group.clone(),
            config.user_secret.clone(),
            config.password_secret.clone(),
            readonly,
        )
        .await
    }

    /// Get the remote database config for this type.  Override this to use a database other than
    /// the primary database.
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&RemoteDatabaseConfig> {
        Some(&remote.primary)
    }

    fn oss_remote_database_config(
        remote: &OssRemoteMetadataDatabaseConfig,
    ) -> Option<&OssRemoteDatabaseConfig> {
        Some(&remote.primary)
    }
}

/// Trait that allows construction of shardable databases from the metadata database config.
#[async_trait::async_trait]
pub trait SqlShardableConstructFromMetadataDatabaseConfig:
    FbSqlConstruct + FbSqlShardedConstruct + SqlConstruct
{
    async fn with_metadata_database_config(
        fb: FacebookInit,
        metadata_database_config: &MetadataDatabaseConfig,
        mysql_options: &MysqlOptions,
        readonly: bool,
    ) -> Result<Self> {
        match metadata_database_config {
            MetadataDatabaseConfig::Local(LocalDatabaseConfig { path }) => {
                Self::with_sqlite_path(path.join("sqlite_dbs"), readonly)
            }
            MetadataDatabaseConfig::Remote(remote) => {
                Self::with_remote_metadata_database_config(fb, remote, mysql_options, readonly)
            }
            MetadataDatabaseConfig::OssRemote(remote) => {
                Self::with_oss_remote_metadata_database_config(fb, remote, readonly).await
            }
        }
    }

    fn with_remote_metadata_database_config(
        fb: FacebookInit,
        remote: &RemoteMetadataDatabaseConfig,
        mysql_options: &MysqlOptions,
        readonly: bool,
    ) -> Result<Self> {
        let config = Self::remote_database_config(remote)
            .ok_or_else(|| anyhow!("no configuration available"))?;
        match config {
            ShardableRemoteDatabaseConfig::Unsharded(config) => {
                Self::with_mysql(fb, config.db_address.clone(), mysql_options, readonly)
            }
            ShardableRemoteDatabaseConfig::Sharded(config) => Self::with_sharded_mysql(
                fb,
                config.shard_map.clone(),
                config.shard_num,
                mysql_options,
                readonly,
            ),
        }
    }

    async fn with_oss_remote_metadata_database_config(
        fb: FacebookInit,
        remote: &OssRemoteMetadataDatabaseConfig,
        readonly: bool,
    ) -> Result<Self> {
        let config = Self::oss_remote_database_config(remote)
            .ok_or_else(|| anyhow!("no configuration available"))?;

        Self::with_oss_mysql(
            fb,
            config.host.clone(),
            config.port,
            config.database.clone(),
            config.secret_group.clone(),
            config.user_secret.clone(),
            config.password_secret.clone(),
            readonly,
        )
        .await
    }

    /// Get the remote database config for this type.
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&ShardableRemoteDatabaseConfig>;

    fn oss_remote_database_config(
        remote: &OssRemoteMetadataDatabaseConfig,
    ) -> Option<&OssRemoteDatabaseConfig> {
        Some(&remote.primary)
    }
}
