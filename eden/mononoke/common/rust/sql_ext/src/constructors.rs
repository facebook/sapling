/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use sql::Connection;
use std::path::Path;

use crate::sqlite::{create_sqlite_connections, open_sqlite_in_memory, open_sqlite_path};

#[derive(Clone)]
pub struct SqlConnections {
    pub write_connection: Connection,
    pub read_connection: Connection,
    pub read_master_connection: Connection,
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
