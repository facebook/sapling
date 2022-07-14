/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use anyhow::Result;
use sql::Connection;
use sql::SqlConnections;
use sql::SqlConnectionsWithSchema;
use sql::SqlShardedConnections;
use sql_ext::open_existing_sqlite_path;
use sql_ext::open_sqlite_in_memory;
use sql_ext::open_sqlite_path;

/// Construct a SQL data manager backed by a database
///
/// This trait should be implemented by any data manager that can be
/// constructed from a single backing database.
pub trait SqlConstruct: Sized + Send + Sync + 'static {
    /// Label used for statistics
    const LABEL: &'static str;

    /// Query used to create an empty instance of the database
    const CREATION_QUERY: &'static str;

    /// Construct an instance from SqlConnections
    ///
    /// This function may be called in an async context and must not block.
    fn from_sql_connections(connections: SqlConnections) -> Self;

    // Wraps from_sql_connections, creating the sqlite schema if required
    fn from_connections_with_schema(connections: SqlConnectionsWithSchema) -> Result<Self> {
        connections.create_schema(Self::CREATION_QUERY)?;
        Ok(Self::from_sql_connections(connections.into()))
    }

    /// Construct an instance from an in-memory SQLite instance
    fn with_sqlite_in_memory() -> Result<Self> {
        let conn = open_sqlite_in_memory()?;
        conn.execute_batch(Self::CREATION_QUERY)?;
        let connections = SqlConnections::new_single(Connection::with_sqlite(conn));
        Ok(Self::from_sql_connections(connections))
    }

    /// Construct an instance from a SQLite database
    fn with_sqlite_path<P: AsRef<Path>>(path: P, readonly: bool) -> Result<Self> {
        let path = path.as_ref();
        let conn = open_sqlite_path(path, false)?;
        conn.execute_batch(Self::CREATION_QUERY)?;
        let write_connection = Connection::with_sqlite(conn);
        let read_connection = Connection::with_sqlite(open_existing_sqlite_path(path, true)?);
        let connections = SqlConnections {
            write_connection: if readonly {
                read_connection.clone()
            } else {
                write_connection
            },
            read_master_connection: read_connection.clone(),
            read_connection,
        };
        Ok(Self::from_sql_connections(connections))
    }
}

/// Construct a SQL data manager backed by a sharded database
///
/// This trait should be implemented by any data manager that can be
/// constructed from a sharded backing database.
pub trait SqlShardedConstruct: Sized + Send + Sync + 'static {
    /// Label used for statistics
    const LABEL: &'static str;

    /// Query used to create an empty instance of a shard
    const CREATION_QUERY: &'static str;

    /// Construct an instance from a vector of SqlConnections, one for each shard
    ///
    /// This function may be called in an async context and must not block.
    fn from_sql_shard_connections(shard_connections: SqlShardedConnections) -> Self;
}
