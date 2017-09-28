// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate url;

#[cfg(test)]
extern crate tempdir;

extern crate blobstore;
extern crate futures_ext;

use std::fs::{create_dir_all, File};
use std::io::{self, Read, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::string::ToString;

use futures::Async;
use futures::future::poll_fn;
use futures_ext::{BoxFuture, FutureExt};
use url::percent_encoding::{percent_encode, DEFAULT_ENCODE_SET};

use blobstore::Blobstore;

#[cfg(test)]
mod test;

const PREFIX: &str = "blob";

mod errors {
    error_chain! {
        errors {
        }

        links {
        }

        foreign_links {
            Io(::std::io::Error);
        }
    }
}

use errors::*;
pub use errors::{Error, ErrorKind};

#[derive(Debug, Clone)]
pub struct Fileblob<K, V> {
    base: PathBuf,
    _phantom: PhantomData<(K, V)>,
}

impl<K, V> Fileblob<K, V>
where
    K: ToString,
{
    pub fn open<P: AsRef<Path>>(base: P) -> Result<Self> {
        let base = base.as_ref();

        if !base.is_dir() {
            bail!("Base {:?} doesn't exist or is not directory", base);
        }

        Ok(Self {
            base: base.to_owned(),
            _phantom: PhantomData,
        })
    }

    pub fn create<P: AsRef<Path>>(base: P) -> Result<Self> {
        let base = base.as_ref();
        create_dir_all(base)?;
        Self::open(base)
    }

    fn path(&self, key: &K) -> PathBuf {
        let key = key.to_string();
        let key = percent_encode(key.as_bytes(), DEFAULT_ENCODE_SET);
        self.base.join(format!("{}-{}", PREFIX, key))
    }
}

impl<K, V> Blobstore for Fileblob<K, V>
where
    K: ToString + Send + 'static,
    V: AsRef<[u8]> + Send + 'static,
{
    type Error = Error;
    type Key = K;
    type ValueIn = V;
    type ValueOut = Vec<u8>;

    type GetBlob = BoxFuture<Option<Self::ValueOut>, Self::Error>;
    type PutBlob = BoxFuture<(), Self::Error>;

    fn get(&self, key: &Self::Key) -> Self::GetBlob {
        let p = self.path(key);

        poll_fn(move || {
            let mut v = Vec::new();
            let ret = match File::open(&p) {
                Err(ref e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
                Ok(mut f) => {
                    f.read_to_end(&mut v)?;
                    Some(v)
                }
            };
            Ok(Async::Ready(ret))
        }).boxify()
    }

    fn put(&self, key: Self::Key, val: Self::ValueIn) -> Self::PutBlob {
        let p = self.path(&key);

        poll_fn(move || {
            File::create(&p)?.write_all(val.as_ref())?;
            Ok(Async::Ready(()))
        }).boxify()
    }
}
