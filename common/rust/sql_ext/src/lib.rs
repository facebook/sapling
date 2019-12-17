/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{Error, Result};
use cloned::cloned;
use futures::{
    future::{loop_fn, ok, Loop},
    Future,
};
use futures_ext::{BoxFuture, FutureExt};
use slog::{info, Logger};
use std::{path::Path, time::Duration};
use tokio_timer::sleep;

use sql::{
    myrouter, raw,
    rusqlite::{Connection as SqliteConnection, OpenFlags as SqliteOpenFlags},
    Connection, Transaction,
};

#[derive(Copy, Clone, Debug)]
pub struct MysqlOptions {
    pub myrouter_port: Option<u16>,
    pub master_only: bool,
}

impl Default for MysqlOptions {
    fn default() -> Self {
        Self {
            myrouter_port: None,
            master_only: false,
        }
    }
}

impl MysqlOptions {
    pub fn myrouter_read_service_type(&self) -> myrouter::ServiceType {
        if self.master_only {
            myrouter::ServiceType::MASTER
        } else {
            myrouter::ServiceType::ANY
        }
    }

    pub fn db_locator_read_instance_requirement(&self) -> raw::InstanceRequirement {
        if self.master_only {
            raw::InstanceRequirement::Master
        } else {
            raw::InstanceRequirement::ReplicaFirst
        }
    }
}

#[derive(Clone)]
pub struct SqlConnections {
    pub write_connection: Connection,
    pub read_connection: Connection,
    pub read_master_connection: Connection,
}

pub struct PoolSizeConfig {
    pub write_pool_size: usize,
    pub read_pool_size: usize,
    pub read_master_pool_size: usize,
}

impl PoolSizeConfig {
    pub fn for_regular_connection() -> Self {
        Self {
            write_pool_size: 1,
            read_pool_size: myrouter::DEFAULT_MAX_NUM_OF_CONCURRENT_CONNECTIONS,
            // For reading from master we need to use less concurrent connections in order to
            // protect the master from being overloaded. The `clone` here means that for write
            // connection we still use the default number of concurrent connections.
            read_master_pool_size: 10,
        }
    }

    pub fn for_sharded_connection() -> Self {
        Self {
            write_pool_size: 1,
            read_pool_size: 1,
            read_master_pool_size: 1,
        }
    }
}

#[must_use]
pub enum TransactionResult {
    Succeeded(Transaction),
    Failed,
}

pub fn create_myrouter_connections(
    tier: String,
    shard_id: Option<usize>,
    port: u16,
    read_service_type: myrouter::ServiceType,
    pool_size_config: PoolSizeConfig,
    label: String,
    readonly: bool,
) -> SqlConnections {
    let mut builder = Connection::myrouter_builder();
    builder.tier(tier, shard_id).port(port);

    builder.tie_break(myrouter::TieBreak::SLAVE_FIRST);

    builder.label(label);

    let read_connection = builder
        .clone()
        .service_type(read_service_type)
        .max_num_of_concurrent_connections(pool_size_config.read_pool_size)
        .build_read_only();

    builder.service_type(myrouter::ServiceType::MASTER);
    let read_master_connection = builder
        .clone()
        .max_num_of_concurrent_connections(pool_size_config.read_master_pool_size)
        .build_read_only();

    let write_connection = if readonly {
        // Myrouter respects readonly, it connects as scriptro
        read_master_connection.clone()
    } else {
        builder
            .max_num_of_concurrent_connections(pool_size_config.write_pool_size)
            .build_read_write()
    };

    SqlConnections {
        write_connection,
        read_connection,
        read_master_connection,
    }
}

fn do_create_raw_xdb_connections<'a, T>(
    tier: &'a T,
    read_instance_requirement: raw::InstanceRequirement,
    readonly: bool,
) -> impl Future<Item = SqlConnections, Error = Error>
where
    T: ?Sized,
    &'a T: AsRef<str>,
{
    // TODO(dtolnay): this needs to be passed down from main instead.
    let fb = *fbinit::FACEBOOK;

    let tier: &str = tier.as_ref();

    let write_connection = if readonly {
        ok(None).left_future()
    } else {
        raw::RawConnection::new_from_tier(
            fb,
            tier,
            raw::InstanceRequirement::Master,
            None,
            None,
            None,
        )
        .map(Some)
        .right_future()
    };

    let read_connection = raw::RawConnection::new_from_tier(
        fb,
        tier,
        read_instance_requirement,
        None,
        None,
        Some("scriptro"),
    );

    let read_master_connection = raw::RawConnection::new_from_tier(
        fb,
        tier,
        raw::InstanceRequirement::Master,
        None,
        None,
        Some("scriptro"),
    );

    write_connection
        .join3(read_connection, read_master_connection)
        .map(|(wr, rd, rm)| SqlConnections {
            write_connection: Connection::Raw(wr.unwrap_or_else(|| rm.clone())),
            read_connection: Connection::Raw(rd),
            read_master_connection: Connection::Raw(rm),
        })
}

pub fn create_raw_xdb_connections(
    tier: String,
    read_instance_requirement: raw::InstanceRequirement,
    readonly: bool,
) -> impl Future<Item = SqlConnections, Error = Error> {
    let max_attempts = 5;

    loop_fn(0, move |i| {
        do_create_raw_xdb_connections(&tier, read_instance_requirement, readonly).then(move |r| {
            let loop_state = if r.is_ok() || i > max_attempts {
                Loop::Break(r)
            } else {
                Loop::Continue(i + 1)
            };
            Ok(loop_state)
        })
    })
    .and_then(|r| r)
}

fn sqlite_setup_connection(con: &SqliteConnection) {
    // By default, when there's a read/write contention, SQLite will not wait,
    // but rather throw a `SQLITE_BUSY` error. See https://www.sqlite.org/lockingv3.html
    // This means that tests will fail in cases when production setup (e.g. one with MySQL)
    // would not. To change that, let's make sqlite wait for some time, before erroring out
    let _ = con.busy_timeout(Duration::from_secs(10));
}

// Open a single sqlite connection to a new in memory database
pub fn open_sqlite_in_memory() -> Result<SqliteConnection> {
    let con = SqliteConnection::open_in_memory()?;
    sqlite_setup_connection(&con);
    Ok(con)
}

// Open a single sqlite connection
pub fn open_sqlite_path<P: AsRef<Path>>(path: P, readonly: bool) -> Result<SqliteConnection> {
    let flags = if readonly {
        SqliteOpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        SqliteOpenFlags::SQLITE_OPEN_READ_WRITE | SqliteOpenFlags::SQLITE_OPEN_CREATE
    };

    let con = SqliteConnection::open_with_flags(path, flags)?;
    sqlite_setup_connection(&con);
    Ok(con)
}

/// Open sqlite connections for use by SqlConstructors::from_sql_connections
pub fn create_sqlite_connections<P: AsRef<Path>>(
    path: P,
    readonly: bool,
) -> Result<SqlConnections> {
    let ro = Connection::with_sqlite(open_sqlite_path(&path, true)?);
    let rw = if readonly {
        ro.clone()
    } else {
        Connection::with_sqlite(open_sqlite_path(&path, readonly)?)
    };
    Ok(SqlConnections {
        write_connection: rw,
        read_connection: ro.clone(),
        read_master_connection: ro,
    })
}

/// Set of useful constructors for Mononoke's sql based data access objects
pub trait SqlConstructors: Sized + Send + Sync + 'static {
    /// Label used for stats accounting, and also for the local DB name
    const LABEL: &'static str;

    fn from_sql_connections(c: SqlConnections) -> Self {
        Self::from_connections(
            c.write_connection,
            c.read_connection,
            c.read_master_connection,
        )
    }

    /// TODO(ahornby) consider removing this
    fn from_connections(
        write_connection: Connection,
        read_connection: Connection,
        read_master_connection: Connection,
    ) -> Self;

    fn get_up_query() -> &'static str;

    fn with_myrouter(
        tier: String,
        port: u16,
        read_service_type: myrouter::ServiceType,
        readonly: bool,
    ) -> Self {
        let r = create_myrouter_connections(
            tier,
            None,
            port,
            read_service_type,
            PoolSizeConfig::for_regular_connection(),
            Self::LABEL.to_string(),
            readonly,
        );
        Self::from_sql_connections(r)
    }

    fn with_raw_xdb_tier(
        tier: String,
        read_instance_requirement: raw::InstanceRequirement,
        readonly: bool,
    ) -> BoxFuture<Self, Error> {
        create_raw_xdb_connections(tier, read_instance_requirement, readonly)
            .map(|r| Self::from_sql_connections(r))
            .boxify()
    }

    fn with_xdb(tier: String, options: MysqlOptions, readonly: bool) -> BoxFuture<Self, Error> {
        match options.myrouter_port {
            Some(myrouter_port) => ok(Self::with_myrouter(
                tier,
                myrouter_port,
                options.myrouter_read_service_type(),
                readonly,
            ))
            .boxify(),
            None => Self::with_raw_xdb_tier(
                tier,
                options.db_locator_read_instance_requirement(),
                readonly,
            ),
        }
    }

    fn with_sqlite_in_memory() -> Result<Self> {
        // In memory never readonly
        let con = open_sqlite_in_memory()?;
        con.execute_batch(Self::get_up_query())?;
        let con = Connection::with_sqlite(con);
        Ok(Self::from_connections(con.clone(), con.clone(), con))
    }

    fn with_sqlite_path<P: AsRef<Path>>(path: P, readonly: bool) -> Result<Self> {
        // Do update on writable connection to construct schema
        let con = open_sqlite_path(&path, false)?;
        // When opening an sqlite database we might already have the proper tables in it, so ignore
        // errors from table creation
        let _ = con.execute_batch(Self::get_up_query());

        create_sqlite_connections(&path, readonly).map(|r| Self::from_sql_connections(r))
    }
}

pub fn myrouter_ready(
    db_addr_opt: Option<String>,
    mysql_options: MysqlOptions,
    logger: Logger,
) -> impl Future<Item = (), Error = Error> {
    let logger_fut = loop_fn((), move |()| {
        cloned!(logger);
        sleep(Duration::from_secs(1)).map(move |_| {
            info!(logger, "waiting for myrouter...");
            Loop::Continue(())
        })
    })
    .from_err();

    let f = match db_addr_opt {
        None => ok(()).left_future(), // No DB required: we can skip myrouter.
        Some(db_address) => {
            if let Some(myrouter_port) = mysql_options.myrouter_port {
                myrouter::wait_for_myrouter(myrouter_port, db_address).right_future()
            } else {
                // Myrouter was not enabled: we don't need to wait for it.
                ok(()).left_future()
            }
        }
    };

    f.select(logger_fut).map(|_| ()).map_err(|(err, _)| err)
}
