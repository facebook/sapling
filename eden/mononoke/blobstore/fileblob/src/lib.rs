/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::convert::TryFrom;
use std::fs::{create_dir_all, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{bail, Error, Result};
use futures_ext::{BoxFuture as BoxFuture01, FutureExt as FutureExt01};
use futures_old::{
    future::{poll_fn as poll_fn01, Future as Future01},
    Async,
};
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};

use blobstore::{Blobstore, BlobstoreGetData, BlobstoreMetadata};
use context::CoreContext;
use mononoke_types::BlobstoreBytes;
use tempfile::NamedTempFile;

const PREFIX: &str = "blob";
/// https://url.spec.whatwg.org/#fragment-percent-encode-set
const FRAGMENT: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'<').add(b'>').add(b'`');
/// https://url.spec.whatwg.org/#path-percent-encode-set
const PATH: &AsciiSet = &FRAGMENT.add(b'#').add(b'?').add(b'{').add(b'}');

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
        let key = percent_encode(key.as_bytes(), PATH);
        self.base.join(format!("{}-{}", PREFIX, key))
    }
}

fn ctime(file: &File) -> Option<i64> {
    let meta = file.metadata().ok()?;
    let ctime = meta.modified().ok()?;
    let ctime_dur = ctime.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    i64::try_from(ctime_dur.as_secs()).ok()
}

impl Blobstore for Fileblob {
    fn get(&self, _ctx: CoreContext, key: String) -> BoxFuture01<Option<BlobstoreGetData>, Error> {
        let p = self.path(&key);

        poll_fn01(move || {
            let mut v = Vec::new();
            let ret = match File::open(&p) {
                Err(ref e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
                Ok(mut f) => {
                    f.read_to_end(&mut v)?;

                    Some(BlobstoreGetData::new(
                        BlobstoreMetadata::new(ctime(&f)),
                        BlobstoreBytes::from_bytes(v),
                    ))
                }
            };
            Ok(Async::Ready(ret))
        })
        .from_err()
        .boxify()
    }

    fn put(&self, _ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture01<(), Error> {
        let p = self.path(&key);

        poll_fn01::<_, Error, _>(move || {
            let tempfile = NamedTempFile::new()?;
            tempfile.as_file().write_all(value.as_bytes().as_ref())?;
            tempfile.persist(&p)?;
            Ok(Async::Ready(()))
        })
        .boxify()
    }

    fn is_present(&self, _ctx: CoreContext, key: String) -> BoxFuture01<bool, Error> {
        let p = self.path(&key);

        poll_fn01(move || {
            let ret = match File::open(&p) {
                Err(ref e) if e.kind() == io::ErrorKind::NotFound => false,
                Err(e) => return Err(e),
                Ok(_) => true,
            };
            Ok(Async::Ready(ret))
        })
        .from_err()
        .boxify()
    }
}
