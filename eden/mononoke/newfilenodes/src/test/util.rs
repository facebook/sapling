/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use sql::rusqlite::Connection as SqliteConnection;
use sql::Connection;
use sql_construct::SqlConstruct;

use crate::builder::NewFilenodesBuilder;
use crate::builder::SQLITE_INSERT_CHUNK_SIZE;
use crate::reader::FilenodesReader;
use crate::writer::FilenodesWriter;

pub fn build_shard() -> Result<Connection, Error> {
    let con = SqliteConnection::open_in_memory()?;
    con.execute_batch(NewFilenodesBuilder::CREATION_QUERY)?;
    Ok(Connection::with_sqlite(con))
}

pub fn build_reader_writer(shards: Vec<Connection>) -> (FilenodesReader, FilenodesWriter) {
    let reader = FilenodesReader::new(shards.clone(), shards.clone());
    let writer = FilenodesWriter::new(SQLITE_INSERT_CHUNK_SIZE, shards.clone(), shards);
    (reader, writer)
}
