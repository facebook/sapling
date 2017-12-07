// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate heads;
extern crate mercurial_types;

#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_cpupool;
extern crate futures_ext;
#[cfg(test)]
extern crate tempdir;

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::string::ToString;
use std::sync::Arc;

use failure::{Error, Result, ResultExt};
use futures::Async;
use futures::future::{poll_fn, Future, IntoFuture};
use futures::stream::{self, Stream};
use futures_cpupool::CpuPool;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use heads::Heads;
use mercurial_types::NodeHash;

static PREFIX: &'static str = "head-";

/// A basic file-based persistent head store.
///
/// Stores heads as empty files in the specified directory. File operations are dispatched to
/// a thread pool to avoid blocking the main thread with IO. For simplicity, file accesses
/// are unsynchronized since each operation performs just a single File IO syscall.
pub struct FileHeads {
    base: PathBuf,
    pool: Arc<CpuPool>,
}

impl FileHeads {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_with_pool(path, Arc::new(CpuPool::new_num_cpus()))
    }

    pub fn open_with_pool<P: AsRef<Path>>(path: P, pool: Arc<CpuPool>) -> Result<Self> {
        let path = path.as_ref();

        if !path.is_dir() {
            bail_msg!("'{}' is not a directory", path.to_string_lossy());
        }

        Ok(FileHeads {
            base: path.to_path_buf(),
            pool: pool,
        })
    }

    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::create_with_pool(path, Arc::new(CpuPool::new_num_cpus()))
    }

    pub fn create_with_pool<P: AsRef<Path>>(path: P, pool: Arc<CpuPool>) -> Result<Self> {
        let path = path.as_ref();
        fs::create_dir_all(path)?;
        Self::open_with_pool(path, pool)
    }

    fn get_path(&self, key: &NodeHash) -> Result<PathBuf> {
        Ok(self.base.join(format!("{}{}", PREFIX, key.to_string())))
    }
}

impl Heads for FileHeads {
    fn add(&self, key: &NodeHash) -> BoxFuture<(), Error> {
        let pool = self.pool.clone();
        self.get_path(&key)
            .into_future()
            .and_then(move |path| {
                let future = poll_fn(move || {
                    File::create(&path)?;
                    Ok(Async::Ready(()))
                });
                pool.spawn(future)
            })
            .boxify()
    }

    fn remove(&self, key: &NodeHash) -> BoxFuture<(), Error> {
        let pool = self.pool.clone();
        self.get_path(&key)
            .into_future()
            .and_then(move |path| {
                let future = poll_fn(move || {
                    fs::remove_file(&path).or_else(|e| {
                        // Don't report an error if the file doesn't exist.
                        match e.kind() {
                            io::ErrorKind::NotFound => Ok(()),
                            _ => Err(e),
                        }
                    })?;
                    Ok(Async::Ready(()))
                });
                pool.spawn(future)
            })
            .boxify()
    }

    fn is_head(&self, key: &NodeHash) -> BoxFuture<bool, Error> {
        let pool = self.pool.clone();
        self.get_path(&key)
            .into_future()
            .and_then(move |path| {
                let future = poll_fn(move || Ok(Async::Ready(path.exists())));
                pool.spawn(future)
            })
            .boxify()
    }

    fn heads(&self) -> BoxStream<NodeHash, Error> {
        let names = fs::read_dir(&self.base).map(|entries| {
            entries
                .map(|result| {
                    result
                        .map_err(From::from)
                        .map(|entry| entry.file_name().to_string_lossy().into_owned())
                })
                .filter_map(|result| match result {
                    Ok(ref name) if name.starts_with(PREFIX) => {
                        let name = &name[PREFIX.len()..];
                        let name = NodeHash::from_str(name)
                            .context("can't parse name")
                            .map_err(Error::from);
                        Some(name)
                    }
                    Ok(_) => None,
                    Err(err) => Some(Err(err)),
                })
        });
        match names {
            Ok(v) => stream::iter_ok(v).and_then(|v| v).boxify(),
            Err(e) => stream::once(Err(e.into())).boxify(),
        }
    }
}


#[cfg(test)]
mod test {
    use super::*;
    use tempdir::TempDir;

    #[test]
    fn invalid_dir() {
        let tmp = TempDir::new("filebookmarks_heads_invalid_dir").unwrap();
        let heads = FileHeads::open(tmp.path().join("does_not_exist"));
        assert!(heads.is_err());
    }
}
