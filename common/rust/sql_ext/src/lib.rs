// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate failure_ext as failure;
extern crate sql;

use std::path::Path;

use failure::prelude::*;
use sql::{myrouter, Connection, rusqlite::Connection as SqliteConnection};

/// Set of useful constructors for Mononoke's sql based data access objects
pub trait SqlConstructors: Sized {
    fn from_connections(
        write_connection: Connection,
        read_connection: Connection,
        read_master_connection: Connection,
    ) -> Self;

    fn get_up_query() -> &'static str;

    fn with_myrouter(tier: impl ToString, port: u16) -> Self {
        let mut builder = Connection::myrouter_builder();
        builder.tier(tier).port(port);

        let read_connection = builder.build_read_only();

        builder.service_type(myrouter::ServiceType::MASTER);
        let read_master_connection = builder.build_read_only();
        let write_connection = builder.build_read_write();

        Self::from_connections(write_connection, read_connection, read_master_connection)
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
