// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate bytes;
#[macro_use]
extern crate error_chain;
extern crate futures;

extern crate blobstore;
extern crate rocksdb;

use std::path::Path;

use bytes::Bytes;

use futures::{Async, Future, Poll};

use rocksdb::{Db, ReadOptions, WriteOptions};

use blobstore::Blobstore;

mod errors;

pub use errors::{Error, ErrorKind, Result, ResultExt};

#[derive(Clone)]
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
pub struct PutBlob(Db, String, Bytes);

impl Future for GetBlob {
    type Item = Option<rocksdb::Buffer>;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let rdopts = ReadOptions::new();
        let ret = self.0.get(&self.1, &rdopts).map_err(Error::from)?;
        Ok(Async::Ready(ret))
    }
}

impl Future for PutBlob {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let wropts = WriteOptions::new().set_sync(false);
        self.0.put(&self.1, &self.2, &wropts).map_err(Error::from)?;
        Ok(Async::Ready(()))
    }
}

impl Blobstore for Rocksblob {
    type ValueIn = Bytes;
    type ValueOut = rocksdb::Buffer;
    type Error = Error;
    // TODO: remove these and use poll_fn once we have `impl Future`
    type GetBlob = GetBlob;
    type PutBlob = PutBlob;

    fn get(&self, key: String) -> Self::GetBlob {
        let db = self.db.clone();

        GetBlob(db, key)
    }

    fn put(&self, key: String, val: Self::ValueIn) -> Self::PutBlob {
        let db = self.db.clone();

        PutBlob(db, key, val)
    }
}
