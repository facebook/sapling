/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use fbinit::FacebookInit;
use futures::{FutureExt, TryFutureExt};
use futures_ext::{BoxFuture, FutureExt as _};
use futures_old::{
    future::{self, IntoFuture},
    Future,
};
use metaconfig_types::{
    LocalDatabaseConfig, MetadataDatabaseConfig, ShardableRemoteDatabaseConfig,
};
use slog::Logger;
use sql::Connection;
use sql_construct::{
    SqlConstructFromMetadataDatabaseConfig, SqlShardableConstructFromMetadataDatabaseConfig,
};
use sql_ext::{
    facebook::{
        create_myrouter_connections, create_raw_xdb_connections, myrouter_ready, MysqlOptions,
        PoolSizeConfig,
    },
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

impl MetadataSqlFactory {
    pub fn open<T: SqlConstructFromMetadataDatabaseConfig>(&self) -> BoxFuture<T, Error> {
        // FIXME: remove boxing and cloning when this crate is migrated to new futures
        let factory = self.clone();
        async move {
            T::with_metadata_database_config(
                factory.fb,
                &factory.dbconfig,
                factory.mysql_options,
                factory.readonly.0,
            )
            .await
        }
        .boxed()
        .compat()
        .boxify()
    }

    pub fn open_shardable<T: SqlShardableConstructFromMetadataDatabaseConfig>(
        &self,
    ) -> BoxFuture<T, Error> {
        // FIXME: remove boxing and cloning when this crate is migrated to new futures
        let factory = self.clone();
        async move {
            T::with_metadata_database_config(
                factory.fb,
                &factory.dbconfig,
                factory.mysql_options,
                factory.readonly.0,
            )
            .await
        }
        .boxed()
        .compat()
        .boxify()
    }

    pub fn tier_name_shardable<T: SqlShardableConstructFromMetadataDatabaseConfig>(
        &self,
    ) -> Result<String, Error> {
        match &self.dbconfig {
            MetadataDatabaseConfig::Local(_) => Ok("sqlite".to_string()),
            MetadataDatabaseConfig::Remote(remote) => match T::remote_database_config(remote) {
                Some(ShardableRemoteDatabaseConfig::Unsharded(config)) => {
                    Ok(config.db_address.clone())
                }
                Some(ShardableRemoteDatabaseConfig::Sharded(config)) => {
                    Ok(config.shard_map.clone())
                }
                None => Err(anyhow!("missing tier name in configuration")),
            },
        }
    }

    /// Make connections to the primary metadata database
    pub fn make_primary_connections(&self, label: String) -> BoxFuture<SqlConnections, Error> {
        match &self.dbconfig {
            MetadataDatabaseConfig::Local(LocalDatabaseConfig { path }) => {
                open_sqlite_path(path.join("sqlite_dbs"), self.readonly.0)
                    .into_future()
                    .map(|conn| SqlConnections::new_single(Connection::with_sqlite(conn)))
                    .boxify()
            }
            MetadataDatabaseConfig::Remote(config) => match self.mysql_options.myrouter_port {
                Some(myrouter_port) => future::ok(create_myrouter_connections(
                    config.primary.db_address.clone(),
                    None,
                    myrouter_port,
                    self.mysql_options.read_connection_type(),
                    PoolSizeConfig::for_regular_connection(),
                    label,
                    self.readonly.0,
                ))
                .boxify(),
                None => create_raw_xdb_connections(
                    self.fb,
                    config.primary.db_address.clone(),
                    self.mysql_options.read_connection_type(),
                    self.readonly.0,
                )
                .boxify(),
            },
        }
    }
}

pub fn make_metadata_sql_factory(
    fb: FacebookInit,
    dbconfig: MetadataDatabaseConfig,
    mysql_options: MysqlOptions,
    readonly: ReadOnlyStorage,
    logger: Logger,
) -> impl Future<Item = MetadataSqlFactory, Error = Error> {
    let ready = match dbconfig.primary_address() {
        Some(dbaddress) => myrouter_ready(Some(dbaddress), mysql_options, logger).left_future(),
        None => future::ok(()).right_future(),
    };
    ready.map(move |()| MetadataSqlFactory {
        fb,
        dbconfig,
        mysql_options,
        readonly,
    })
}
