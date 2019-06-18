// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate failure_ext as failure;
extern crate sql;

use failure::{Error, Result};
use futures::{future::ok, Future};
use futures_ext::FutureExt;
use std::path::Path;

use sql::{myrouter, raw, rusqlite::Connection as SqliteConnection, Connection};

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
    fn for_regular_connection() -> Self {
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

pub fn create_myrouter_connections(
    tier: impl ToString,
    port: u16,
    pool_size_config: PoolSizeConfig,
    label: impl ToString,
) -> SqlConnections {
    let mut builder = Connection::myrouter_builder();
    builder.tier(tier).port(port);

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

pub fn create_raw_xdb_connections(tier: impl ToString) -> Result<SqlConnections> {
    let tier = tier.to_string();

    let write_connection = Connection::Raw(raw::RawConnection::new_from_tier(
        &tier,
        raw::InstanceRequirement::Master,
        None,
        None,
    )?);

    let read_connection = Connection::Raw(raw::RawConnection::new_from_tier(
        &tier,
        raw::InstanceRequirement::ReplicaFirst,
        None,
        None,
    )?);

    let read_master_connection = Connection::Raw(raw::RawConnection::new_from_tier(
        &tier,
        raw::InstanceRequirement::Master,
        None,
        None,
    )?);

    Ok(SqlConnections {
        write_connection,
        read_connection,
        read_master_connection,
    })
}

/// Set of useful constructors for Mononoke's sql based data access objects
pub trait SqlConstructors: Sized {
    /// Label used for stats accounting, and also for the local DB name
    const LABEL: &'static str;

    fn from_connections(
        write_connection: Connection,
        read_connection: Connection,
        read_master_connection: Connection,
    ) -> Self;

    fn get_up_query() -> &'static str;

    fn with_myrouter(tier: impl ToString, port: u16) -> Self {
        let SqlConnections {
            write_connection,
            read_connection,
            read_master_connection,
        } = create_myrouter_connections(
            tier,
            port,
            PoolSizeConfig::for_regular_connection(),
            Self::LABEL,
        );

        Self::from_connections(write_connection, read_connection, read_master_connection)
    }

    fn with_raw_xdb_tier(tier: impl ToString) -> Result<Self> {
        let SqlConnections {
            write_connection,
            read_connection,
            read_master_connection,
        } = create_raw_xdb_connections(tier)?;

        Ok(Self::from_connections(
            write_connection,
            read_connection,
            read_master_connection,
        ))
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
        with_sqlite(con)
    }
}

fn with_sqlite<T: SqlConstructors>(con: SqliteConnection) -> Result<T> {
    let con = Connection::with_sqlite(con);
    Ok(T::from_connections(con.clone(), con.clone(), con))
}

pub fn myrouter_ready(
    db_addr_opt: Option<&str>,
    myrouter_port: Option<u16>,
) -> impl Future<Item = (), Error = Error> {
    match db_addr_opt {
        None => ok(()).left_future(), // No DB required: we can skip myrouter.
        Some(db_address) => {
            if let Some(myrouter_port) = myrouter_port {
                myrouter::wait_for_myrouter(myrouter_port, db_address).right_future()
            } else {
                // Myrouter was not enabled: we don't need to wait for it.
                ok(()).left_future()
            }
        }
    }
}
