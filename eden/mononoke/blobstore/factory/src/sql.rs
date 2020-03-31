/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use cloned::cloned;
use fbinit::FacebookInit;
use futures::{
    future::{self, IntoFuture},
    Future,
};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use itertools::Either;
use metaconfig_types::{MetadataDBConfig, ShardedFilenodesParams};
use newfilenodes::NewFilenodesBuilder;
use slog::Logger;
use sql_ext::{
    create_sqlite_connections,
    facebook::{
        create_myrouter_connections, create_raw_xdb_connections, myrouter_ready, FbSqlConstructors,
        MysqlOptions, PoolSizeConfig,
    },
    SqlConnections, SqlConstructors,
};
use std::{path::PathBuf, sync::Arc};

use crate::ReadOnlyStorage;

trait SqlFactoryBase: Send + Sync {
    /// Open an arbitrary struct implementing SqlConstructors
    fn open<T: SqlConstructors>(&self) -> BoxFuture<Arc<T>, Error> {
        self.open_owned().map(|r| Arc::new(r)).boxify()
    }

    /// Open an arbitrary struct implementing SqlConstructors (without Arc)
    fn open_owned<T: SqlConstructors>(&self) -> BoxFuture<T, Error>;

    /// Open NewFilenodesBuilder, and return a tier name and the struct.
    fn open_filenodes(&self) -> BoxFuture<(String, NewFilenodesBuilder), Error>;

    /// Creates connections to the db.
    fn create_connections(&self, label: String) -> BoxFuture<SqlConnections, Error>;
}

struct XdbFactory {
    fb: FacebookInit,
    db_address: String,
    readonly: bool,
    mysql_options: MysqlOptions,
    sharded_filenodes: Option<ShardedFilenodesParams>,
}

impl XdbFactory {
    fn new(
        fb: FacebookInit,
        db_address: String,
        mysql_options: MysqlOptions,
        sharded_filenodes: Option<ShardedFilenodesParams>,
        readonly: bool,
    ) -> Self {
        XdbFactory {
            fb,
            db_address,
            readonly,
            mysql_options,
            sharded_filenodes,
        }
    }
}

impl SqlFactoryBase for XdbFactory {
    fn open_owned<T: SqlConstructors>(&self) -> BoxFuture<T, Error> {
        T::with_xdb(
            self.fb,
            self.db_address.clone(),
            self.mysql_options,
            self.readonly,
        )
        .boxify()
    }

    fn open_filenodes(&self) -> BoxFuture<(String, NewFilenodesBuilder), Error> {
        let (tier, filenodes) = match self.sharded_filenodes.clone() {
            Some(ShardedFilenodesParams {
                shard_map,
                shard_num,
            }) => {
                let builder = NewFilenodesBuilder::with_sharded_xdb(
                    self.fb,
                    shard_map.clone(),
                    self.mysql_options,
                    shard_num.into(),
                    self.readonly,
                );
                (shard_map, builder)
            }
            None => {
                let builder = NewFilenodesBuilder::with_xdb(
                    self.fb,
                    self.db_address.clone(),
                    self.mysql_options,
                    self.readonly,
                );
                (self.db_address.clone(), builder)
            }
        };

        filenodes.map(move |filenodes| (tier, filenodes)).boxify()
    }

    fn create_connections(&self, label: String) -> BoxFuture<SqlConnections, Error> {
        match self.mysql_options.myrouter_port {
            Some(mysql_options) => future::ok(create_myrouter_connections(
                self.db_address.clone(),
                None,
                mysql_options,
                self.mysql_options.read_connection_type(),
                PoolSizeConfig::for_regular_connection(),
                label,
                self.readonly,
            ))
            .boxify(),
            None => create_raw_xdb_connections(
                self.fb,
                self.db_address.clone(),
                self.mysql_options.read_connection_type(),
                self.readonly,
            )
            .boxify(),
        }
    }
}

struct SqliteFactory {
    path: PathBuf,
    readonly: bool,
}

impl SqliteFactory {
    fn new(path: PathBuf, readonly: bool) -> Self {
        SqliteFactory { path, readonly }
    }
}

impl SqlFactoryBase for SqliteFactory {
    fn open_owned<T: SqlConstructors>(&self) -> BoxFuture<T, Error> {
        let r = try_boxfuture!(T::with_sqlite_path(
            self.path.join("sqlite_dbs"),
            self.readonly
        ));
        Ok(r).into_future().boxify()
    }

    fn open_filenodes(&self) -> BoxFuture<(String, NewFilenodesBuilder), Error> {
        NewFilenodesBuilder::with_sqlite_path(self.path.join("sqlite_dbs"), self.readonly)
            .map(|filenodes| ("sqlite".to_string(), filenodes))
            .into_future()
            .boxify()
    }

    fn create_connections(&self, _label: String) -> BoxFuture<SqlConnections, Error> {
        create_sqlite_connections(&self.path.join("sqlite_dbs"), self.readonly)
            .into_future()
            .boxify()
    }
}

pub struct SqlFactory {
    underlying: Either<SqliteFactory, XdbFactory>,
}

impl SqlFactory {
    pub fn open<T: SqlConstructors>(&self) -> BoxFuture<Arc<T>, Error> {
        self.underlying.as_ref().either(|l| l.open(), |r| r.open())
    }

    pub fn open_owned<T: SqlConstructors>(&self) -> BoxFuture<T, Error> {
        self.underlying
            .as_ref()
            .either(|l| l.open_owned(), |r| r.open_owned())
    }

    pub fn open_filenodes(&self) -> BoxFuture<(String, NewFilenodesBuilder), Error> {
        self.underlying
            .as_ref()
            .either(|l| l.open_filenodes(), |r| r.open_filenodes())
    }

    pub fn create_connections(&self, label: String) -> BoxFuture<SqlConnections, Error> {
        self.underlying.as_ref().either(
            {
                cloned!(label);
                move |l| l.create_connections(label)
            },
            |r| r.create_connections(label),
        )
    }
}

pub fn make_sql_factory(
    fb: FacebookInit,
    dbconfig: MetadataDBConfig,
    mysql_options: MysqlOptions,
    readonly: ReadOnlyStorage,
    logger: Logger,
) -> impl Future<Item = SqlFactory, Error = Error> {
    match dbconfig {
        MetadataDBConfig::LocalDB { path } => {
            let sql_factory = SqliteFactory::new(path.to_path_buf(), readonly.0);
            future::ok(SqlFactory {
                underlying: Either::Left(sql_factory),
            })
            .left_future()
        }
        MetadataDBConfig::Mysql {
            db_address,
            sharded_filenodes,
        } => {
            let sql_factory = XdbFactory::new(
                fb,
                db_address.clone(),
                mysql_options,
                sharded_filenodes,
                readonly.0,
            );
            myrouter_ready(Some(db_address), mysql_options, logger)
                .map(move |()| SqlFactory {
                    underlying: Either::Right(sql_factory),
                })
                .right_future()
        }
    }
}
