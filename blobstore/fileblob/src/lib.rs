/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use std::fs::{create_dir_all, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use failure_ext::{bail, Error, Result};
use futures::future::{poll_fn, Future};
use futures::Async;
use futures_ext::{BoxFuture, FutureExt};
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};

use blobstore::Blobstore;
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

impl Blobstore for Fileblob {
    fn get(&self, _ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let p = self.path(&key);

        poll_fn(move || {
            let mut v = Vec::new();
            let ret = match File::open(&p) {
                Err(ref e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
                Ok(mut f) => {
                    f.read_to_end(&mut v)?;
                    Some(BlobstoreBytes::from_bytes(v))
                }
            };
            Ok(Async::Ready(ret))
        })
        .from_err()
        .boxify()
    }

    fn put(&self, _ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        let p = self.path(&key);

        poll_fn::<_, Error, _>(move || {
            let tempfile = NamedTempFile::new()?;
            tempfile.as_file().write_all(value.as_bytes().as_ref())?;
            tempfile.persist(&p)?;
            Ok(Async::Ready(()))
        })
        .boxify()
    }

    fn is_present(&self, _ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        let p = self.path(&key);

        poll_fn(move || {
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
