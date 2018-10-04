// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::vec::Vec;

use bytes::Bytes;
use db_conn::MysqlConnInner;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, PooledConnection};
use failure::Error;
use futures::Future;
use futures_ext::{asynchronize, BoxFuture, FutureExt};

use blobstore::Blobstore;
use mercurial_types::RepositoryId;
use mononoke_types::BlobstoreBytes;

use errors::*;

mod schema;
mod models;

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
pub struct MysqlStreamingChunksFetcher {
    inner: MysqlConnInner,
}

impl MysqlStreamingChunksFetcher {
    fn from(inner: MysqlConnInner) -> Self {
        Self { inner } // one true constructor
    }

    pub fn open(db_address: &str) -> Result<Self> {
        Ok(Self::from(MysqlConnInner::open(db_address)?))
    }

    fn get_conn(&self) -> Result<PooledConnection<ConnectionManager<MysqlConnection>>> {
        self.inner.get_conn()
    }

    pub fn fetch_changelog(
        &self,
        repo: RepositoryId,
        blobstore: impl Blobstore + Clone,
    ) -> BoxFuture<RevlogStreamingChunks, Error> {
        let db = self.clone();

        asynchronize(move || {
            use self::schema::streaming_changelog_chunks;

            let connection = &db.get_conn()?;
            let rows = streaming_changelog_chunks::table
                .filter(streaming_changelog_chunks::repo_id.eq(repo))
                .order(streaming_changelog_chunks::chunk_num.asc())
                .load::<self::models::StreamingChangelogChunksRow>(connection)
                .map_err(Error::from);

            rows.map({
                cloned!(blobstore);
                move |rows| {
                    rows.into_iter().fold(
                        RevlogStreamingChunks::new(),
                        move |mut res, row: self::models::StreamingChangelogChunksRow| {
                            res.data_size += row.data_size as usize;
                            res.index_size += row.idx_size as usize;
                            let data_blob_key =
                                String::from_utf8_lossy(&row.data_blob_name).into_owned();
                            res.data_blobs.push(
                                blobstore
                                    .get(data_blob_key.clone())
                                    .and_then(|data| {
                                        data.ok_or(
                                            ErrorKind::MissingStreamingBlob(data_blob_key).into(),
                                        )
                                    })
                                    .map(BlobstoreBytes::into_bytes)
                                    .boxify(),
                            );
                            let idx_blob_key =
                                String::from_utf8_lossy(&row.idx_blob_name).into_owned();
                            res.index_blobs.push(
                                blobstore
                                    .get(idx_blob_key.clone())
                                    .and_then(|data| {
                                        data.ok_or(
                                            ErrorKind::MissingStreamingBlob(idx_blob_key).into(),
                                        )
                                    })
                                    .map(BlobstoreBytes::into_bytes)
                                    .boxify(),
                            );
                            res
                        },
                    )
                }
            })
        }).boxify()
    }
}
