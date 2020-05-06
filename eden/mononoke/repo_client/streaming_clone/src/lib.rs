/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::vec::Vec;

use anyhow::Error;
use bytes::Bytes;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use sql::{queries, Connection};
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::SqlConnections;
use thiserror::Error;

use blobstore::Blobstore;
use context::CoreContext;
use mononoke_types::RepositoryId;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("missing blob {0}")]
    MissingStreamingBlob(String),
    #[error("incorrect size {1} (expected {2}) of corrupt blob {0}")]
    CorruptStreamingBlob(String, usize, usize),
}

pub struct RevlogStreamingChunks {
    pub index_size: usize,
    pub data_size: usize,
    pub index_blobs: Vec<BoxFuture<Bytes, Error>>,
    pub data_blobs: Vec<BoxFuture<Bytes, Error>>,
}

impl RevlogStreamingChunks {
    pub fn new() -> Self {
        Self {
            data_size: 0,
            index_size: 0,
            data_blobs: Vec::new(),
            index_blobs: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct SqlStreamingChunksFetcher {
    read_connection: Connection,
}

queries! {
    read SelectChunks(repo_id: RepositoryId) -> (Vec<u8>, i32, Vec<u8>, i32) {
        "SELECT idx_blob_name, idx_size, data_blob_name, data_size
         FROM streaming_changelog_chunks
         WHERE repo_id = {repo_id}
         ORDER BY chunk_num ASC"
    }
}

impl SqlConstruct for SqlStreamingChunksFetcher {
    const LABEL: &'static str = "streaming-chunks";

    const CREATION_QUERY: &'static str = "";

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            read_connection: connections.read_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlStreamingChunksFetcher {}

fn fetch_blob<B: Blobstore>(
    ctx: CoreContext,
    blobstore: &B,
    key: &[u8],
    expected_size: usize,
) -> BoxFuture<Bytes, Error> {
    let key = String::from_utf8_lossy(key).into_owned();
    blobstore
        .get(ctx.clone(), key.clone())
        .and_then(move |data| match data {
            None => Err(ErrorKind::MissingStreamingBlob(key).into()),
            Some(data) if data.as_bytes().len() == expected_size => Ok(data.into_raw_bytes()),
            Some(data) => {
                Err(
                    ErrorKind::CorruptStreamingBlob(key, data.as_bytes().len(), expected_size)
                        .into(),
                )
            }
        })
        .boxify()
}

impl SqlStreamingChunksFetcher {
    pub fn fetch_changelog(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        blobstore: impl Blobstore + Clone,
    ) -> BoxFuture<RevlogStreamingChunks, Error> {
        SelectChunks::query(&self.read_connection, &repo_id)
            .map(move |rows| {
                rows.into_iter().fold(
                    RevlogStreamingChunks::new(),
                    move |mut res, (idx_blob_name, idx_size, data_blob_name, data_size)| {
                        let data_size = data_size as usize;
                        let idx_size = idx_size as usize;
                        res.data_size += data_size;
                        res.index_size += idx_size;
                        res.data_blobs.push(fetch_blob(
                            ctx.clone(),
                            &blobstore,
                            &data_blob_name,
                            data_size,
                        ));
                        res.index_blobs.push(fetch_blob(
                            ctx.clone(),
                            &blobstore,
                            &idx_blob_name,
                            idx_size,
                        ));
                        res
                    },
                )
            })
            .boxify()
    }
}
