/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::convert::TryFrom;
use std::fs::create_dir_all;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{bail, Error, Result};
use futures::future::{BoxFuture, FutureExt, TryFutureExt};
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};

use blobstore::{Blobstore, BlobstoreGetData, BlobstoreMetadata, BlobstoreWithLink};
use context::CoreContext;
use mononoke_types::BlobstoreBytes;
use tempfile::NamedTempFile;
use tokio::{
    fs::{hard_link, File},
    io::{self, AsyncReadExt, AsyncWriteExt},
};

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

async fn ctime(file: &File) -> Option<i64> {
    let meta = file.metadata().await.ok()?;
    let ctime = meta.modified().ok()?;
    let ctime_dur = ctime.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    i64::try_from(ctime_dur.as_secs()).ok()
}

impl Blobstore for Fileblob {
    fn get(
        &self,
        _ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        let p = self.path(&key);

        async move {
            let ret = match File::open(&p).await {
                Err(ref r) if r.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
                Ok(mut f) => {
                    let mut v = Vec::new();
                    f.read_to_end(&mut v).await?;

                    Some(BlobstoreGetData::new(
                        BlobstoreMetadata::new(ctime(&f).await),
                        BlobstoreBytes::from_bytes(v),
                    ))
                }
            };
            Ok(ret)
        }
        .boxed()
    }

    fn put(
        &self,
        _ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        let p = self.path(&key);

        async move {
            // block_in_place on tempfile would be ideal here, but it interacts
            // badly with tokio_compat
            let tempfile = NamedTempFile::new()?;
            let new_file = tempfile.as_file().try_clone()?;
            let mut tokio_file = File::from_std(new_file);
            tokio_file.write_all(value.as_bytes().as_ref()).await?;
            tempfile.persist(&p)?;
            Ok(())
        }
        .boxed()
    }

    fn is_present(
        &self,
        _ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<bool, Error>> {
        let p = self.path(&key);

        async move {
            let ret = match File::open(&p).await {
                Err(ref e) if e.kind() == io::ErrorKind::NotFound => false,
                Err(e) => return Err(e.into()),
                Ok(_) => true,
            };
            Ok(ret)
        }
        .boxed()
    }
}

impl BlobstoreWithLink for Fileblob {
    // This uses hardlink semantics as the production blobstores also have hardlink like semantics
    // (i.e. you can't discover a canonical link source when loading by the target)
    fn link(
        &self,
        _ctx: CoreContext,
        existing_key: String,
        link_key: String,
    ) -> BoxFuture<'static, Result<(), Error>> {
        // from std::fs::hard_link: The dst path will be a link pointing to the src path
        let src_path = self.path(&existing_key);
        let dst_path = self.path(&link_key);
        hard_link(src_path, dst_path).map_err(Error::from).boxed()
    }
}
