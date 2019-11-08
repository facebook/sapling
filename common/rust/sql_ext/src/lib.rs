/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use cloned::cloned;
use failure_ext::{Error, Result};
use futures::{
    future::{loop_fn, ok, Loop},
    Future, IntoFuture,
};
use futures_ext::{BoxFuture, FutureExt};
use slog::{info, Logger};
use std::{path::Path, time::Duration};
use tokio_timer::sleep;

use sql::{myrouter, raw, rusqlite::Connection as SqliteConnection, Connection, Transaction};

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
    pool_size_config: PoolSizeConfig,
    label: String,
) -> SqlConnections {
    let mut builder = Connection::myrouter_builder();
    builder.tier(tier, shard_id).port(port);

    builder.tie_break(myrouter::TieBreak::SLAVE_FIRST);

    builder.label(label);
    let read_connection = builder
        .max_num_of_concurrent_connections(pool_size_config.read_pool_size)
        .build_read_only();

    builder.service_type(myrouter::ServiceType::MASTER);
    let read_master_connection = builder
        .clone()
        .max_num_of_concurrent_connections(pool_size_config.read_master_pool_size)
        .build_read_only();

    let write_connection = builder
        .max_num_of_concurrent_connections(pool_size_config.write_pool_size)
        .build_read_write();

    SqlConnections {
        write_connection,
        read_connection,
        read_master_connection,
    }
}

pub fn do_create_raw_xdb_connections<'a, T>(
    tier: &'a T,
) -> impl Future<Item = SqlConnections, Error = Error>
where
    T: ?Sized,
    &'a T: AsRef<str>,
{
    // TODO(dtolnay): this needs to be passed down from main instead.
    let fb = *fbinit::FACEBOOK;

    let tier: &str = tier.as_ref();

    let write_connection = raw::RawConnection::new_from_tier(
        fb,
        tier,
        raw::InstanceRequirement::Master,
        None,
        None,
        None,
    );

    let read_connection = raw::RawConnection::new_from_tier(
        fb,
        tier,
        raw::InstanceRequirement::ReplicaFirst,
        None,
        None,
        None,
    );

    let read_master_connection = raw::RawConnection::new_from_tier(
        fb,
        tier,
        raw::InstanceRequirement::Master,
        None,
        None,
        None,
    );

    (write_connection, read_connection, read_master_connection)
        .into_future()
        .map(|(wr, rd, rm)| SqlConnections {
            write_connection: Connection::Raw(wr),
            read_connection: Connection::Raw(rd),
            read_master_connection: Connection::Raw(rm),
        })
}

pub fn create_raw_xdb_connections(
    tier: String,
) -> impl Future<Item = SqlConnections, Error = Error> {
    let max_attempts = 5;

    loop_fn(0, move |i| {
        do_create_raw_xdb_connections(&tier).then(move |r| {
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

pub fn create_sqlite_connections(path: &Path) -> Result<SqlConnections> {
    let con = SqliteConnection::open(path)?;
    let con = Connection::with_sqlite(con);
    Ok(SqlConnections {
        write_connection: con.clone(),
        read_connection: con.clone(),
        read_master_connection: con.clone(),
    })
}

/// Set of useful constructors for Mononoke's sql based data access objects
pub trait SqlConstructors: Sized + Send + Sync + 'static {
    /// Label used for stats accounting, and also for the local DB name
    const LABEL: &'static str;

    fn from_connections(
        write_connection: Connection,
        read_connection: Connection,
        read_master_connection: Connection,
    ) -> Self;

    fn get_up_query() -> &'static str;

    fn with_myrouter(tier: String, port: u16) -> Self {
        let SqlConnections {
            write_connection,
            read_connection,
            read_master_connection,
        } = create_myrouter_connections(
            tier,
            None,
            port,
            PoolSizeConfig::for_regular_connection(),
            Self::LABEL.to_string(),
        );

        Self::from_connections(write_connection, read_connection, read_master_connection)
    }

    fn with_raw_xdb_tier(tier: String) -> BoxFuture<Self, Error> {
        create_raw_xdb_connections(tier)
            .map(|r| {
                let SqlConnections {
                    write_connection,
                    read_connection,
                    read_master_connection,
                } = r;

                Self::from_connections(write_connection, read_connection, read_master_connection)
            })
            .boxify()
    }

    fn with_xdb(tier: String, port: Option<u16>) -> BoxFuture<Self, Error> {
        match port {
            Some(myrouter_port) => ok(Self::with_myrouter(tier, myrouter_port)).boxify(),
            None => Self::with_raw_xdb_tier(tier),
        }
    }

    fn with_sqlite_in_memory() -> Result<Self> {
        let con = SqliteConnection::open_in_memory()?;
        con.execute_batch(Self::get_up_query())?;
        with_sqlite(con)
    }

    fn with_sqlite_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let con = SqliteConnection::open(path)?;
        // When opening an sqlite database we might already have the proper tables in it, so ignore
        // errors from table creation
        let _ = con.execute_batch(Self::get_up_query());
        // By default, when there's a read/write contention, SQLite will not wait,
        // but rather throw a `SQLITE_BUSY` error. See https://www.sqlite.org/lockingv3.html
        // This means that tests will fail in cases when production setup (e.g. one with MySQL)
        // would not. To change that, let's make sqlite wait for some time, before erroring out
        let _ = con.busy_timeout(Duration::from_secs(10));
        with_sqlite(con)
    }
}

fn with_sqlite<T: SqlConstructors>(con: SqliteConnection) -> Result<T> {
    let con = Connection::with_sqlite(con);
    Ok(T::from_connections(con.clone(), con.clone(), con))
}

pub fn myrouter_ready(
    db_addr_opt: Option<String>,
    myrouter_port: Option<u16>,
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
            if let Some(myrouter_port) = myrouter_port {
                myrouter::wait_for_myrouter(myrouter_port, db_address).right_future()
            } else {
                // Myrouter was not enabled: we don't need to wait for it.
                ok(()).left_future()
            }
        }
    };

    f.select(logger_fut).map(|_| ()).map_err(|(err, _)| err)
}
