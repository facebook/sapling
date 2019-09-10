// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use std::path::Path;

use failure_ext as failure;

use crate::failure::Error;
use futures::{Async, Future, Poll};
use futures_ext::{BoxFuture, FutureExt};

use rocksdb::{Db, ReadOptions, WriteOptions};

use blobstore::Blobstore;
use context::CoreContext;
use mononoke_types::BlobstoreBytes;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug)]
pub struct Rocksblob {
    db: Db,
}

impl Rocksblob {
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_with_options(path, rocksdb::Options::new().create_if_missing(true))
    }

    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_with_options(path, rocksdb::Options::new())
    }

    pub fn open_with_options<P: AsRef<Path>>(path: P, opts: rocksdb::Options) -> Result<Self> {
        let opts = opts.set_compression(rocksdb::Compression::Zstd);
        let opts = opts.set_block_based_table_factory(
            &rocksdb::BlockBasedTableOptions::new()
                .set_filter_policy(rocksdb::FilterPolicy::create_bloom(10)),
        );

        Ok(Rocksblob {
            db: Db::open(path, opts)?,
        })
    }
}

#[must_use = "futures do nothing unless polled"]
pub struct GetBlob(Db, String);

#[must_use = "futures do nothing unless polled"]
pub struct PutBlob(Db, String, BlobstoreBytes);

impl Future for GetBlob {
    type Item = Option<BlobstoreBytes>;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let rdopts = ReadOptions::new();
        let ret = self.0.get(&self.1, &rdopts).map_err(Error::from)?;
        Ok(Async::Ready(ret.map(BlobstoreBytes::from_bytes)))
    }
}

impl Future for PutBlob {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let wropts = WriteOptions::new().set_sync(false);
        self.0
            .put(&self.1, &self.2.as_bytes(), &wropts)
            .map_err(Error::from)?;
        Ok(Async::Ready(()))
    }
}

impl Blobstore for Rocksblob {
    fn get(&self, _ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let db = self.db.clone();

        GetBlob(db, key).boxify()
    }

    fn put(&self, _ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        let db = self.db.clone();

        PutBlob(db, key, value).boxify()
    }
}
