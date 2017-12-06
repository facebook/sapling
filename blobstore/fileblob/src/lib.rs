// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate bytes;
#[macro_use]
extern crate failure;
extern crate futures;
extern crate url;

extern crate blobstore;
extern crate futures_ext;

use std::fs::{create_dir_all, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use bytes::Bytes;
use failure::Error;
use futures::Async;
use futures::future::poll_fn;
use futures_ext::{BoxFuture, FutureExt};
use url::percent_encoding::{percent_encode, DEFAULT_ENCODE_SET};

use blobstore::Blobstore;

const PREFIX: &str = "blob";

pub type Result<T> = std::result::Result<T, Error>;

macro_rules! bail {
    ($($arg:expr),*) => {
        return Err(format_err!($($arg),*))
    }
}

#[derive(Debug, Clone)]
pub struct Fileblob {
    base: PathBuf,
}

impl Fileblob {
    pub fn open<P: AsRef<Path>>(base: P) -> Result<Self> {
        let base = base.as_ref();

        if !base.is_dir() {
            bail!("Base {:?} doesn't exist or is not directory", base);
        }

        Ok(Self {
            base: base.to_owned(),
        })
    }

    pub fn create<P: AsRef<Path>>(base: P) -> Result<Self> {
        let base = base.as_ref();
        create_dir_all(base)?;
        Self::open(base)
    }

    fn path(&self, key: &String) -> PathBuf {
        let key = percent_encode(key.as_bytes(), DEFAULT_ENCODE_SET);
        self.base.join(format!("{}-{}", PREFIX, key))
    }
}

impl Blobstore for Fileblob {
    type GetBlob = BoxFuture<Option<Bytes>, Error>;
    type PutBlob = BoxFuture<(), Error>;

    fn get(&self, key: String) -> Self::GetBlob {
        let p = self.path(&key);

        poll_fn(move || {
            let mut v = Vec::new();
            let ret = match File::open(&p) {
                Err(ref e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
                Ok(mut f) => {
                    f.read_to_end(&mut v)?;
                    Some(Bytes::from(v))
                }
            };
            Ok(Async::Ready(ret))
        }).boxify()
    }

    fn put(&self, key: String, val: Bytes) -> Self::PutBlob {
        let p = self.path(&key);

        poll_fn(move || {
            File::create(&p)?.write_all(val.as_ref())?;
            Ok(Async::Ready(()))
        }).boxify()
    }
}
