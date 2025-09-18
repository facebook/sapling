/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use sql::rusqlite::Connection as SqliteConnection;
use sql::sqlite::SqliteCallbacks;
use sql::sqlite::SqliteHlcProvider;
use sql_construct::SqlConstruct;
use sql_ext::Connection;
use vec1::Vec1;

use crate::builder::NewFilenodesBuilder;
use crate::builder::SQLITE_INSERT_CHUNK_SIZE;
use crate::reader::FilenodesReader;
use crate::writer::FilenodesWriter;

pub fn build_shard() -> Result<Connection, Error> {
    let con = SqliteConnection::open_in_memory()?;
    con.execute_batch(NewFilenodesBuilder::CREATION_QUERY)?;
    Connection::with_sqlite(con)
}

pub fn build_shard_with_hlc_provider(
    hlc_provider: Arc<Box<SqliteHlcProvider>>,
    callbacks: Box<dyn SqliteCallbacks>,
) -> Result<Connection, Error> {
    let con = SqliteConnection::open_in_memory()?;
    con.execute_batch(NewFilenodesBuilder::CREATION_QUERY)?;
    Connection::with_sqlite_hlc_provider_and_callbacks(con, hlc_provider, callbacks)
}

pub fn build_shard_with_callbacks(
    callbacks: Box<dyn SqliteCallbacks>,
) -> Result<Connection, Error> {
    let con = SqliteConnection::open_in_memory()?;
    con.execute_batch(NewFilenodesBuilder::CREATION_QUERY)?;
    Connection::with_sqlite_callbacks(con, callbacks)
}

pub fn build_reader_writer(shards: Vec1<Connection>) -> (FilenodesReader, FilenodesWriter) {
    let reader = FilenodesReader::new(shards.clone(), shards.clone()).unwrap();
    let writer = FilenodesWriter::new(SQLITE_INSERT_CHUNK_SIZE, shards.clone(), shards);
    (reader, writer)
}
