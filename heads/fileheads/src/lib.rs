// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
// TODO: (sid0) T21726029 tokio/futures deprecated a bunch of stuff, clean it all up
#![allow(deprecated)]

extern crate heads;

#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate futures_cpupool;
#[cfg(test)]
extern crate mercurial_types;
#[cfg(test)]
extern crate tempdir;

use std::error;
use std::fs::{self, File};
use std::io;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::string::ToString;
use std::sync::Arc;

use futures::Async;
use futures::future::{poll_fn, BoxFuture, Future, IntoFuture};
use futures::stream::{self, BoxStream, Stream};
use futures_cpupool::CpuPool;

use heads::Heads;

mod errors {
    error_chain!{
        foreign_links {
            Io(::std::io::Error);
        }
    }
}
pub use errors::*;

static PREFIX: &'static str = "head-";

/// A basic file-based persistent head store.
///
/// Stores heads as empty files in the specified directory. File operations are dispatched to
/// a thread pool to avoid blocking the main thread with IO. For simplicity, file accesses
/// are unsynchronized since each operation performs just a single File IO syscall.
pub struct FileHeads<T> {
    base: PathBuf,
    pool: Arc<CpuPool>,
    _marker: PhantomData<T>,
}

impl<T> FileHeads<T>
where
    T: ToString + Send,
{
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_with_pool(path, Arc::new(CpuPool::new_num_cpus()))
    }

    pub fn open_with_pool<P: AsRef<Path>>(path: P, pool: Arc<CpuPool>) -> Result<Self> {
        let path = path.as_ref();

        if !path.is_dir() {
            bail!("'{}' is not a directory", path.to_string_lossy());
        }

        Ok(FileHeads {
            base: path.to_path_buf(),
            pool: pool,
            _marker: PhantomData,
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

    fn get_path(&self, key: &T) -> Result<PathBuf> {
        Ok(self.base.join(format!("{}{}", PREFIX, key.to_string())))
    }
}

impl<T> Heads for FileHeads<T>
where
    T: FromStr + ToString + Send + 'static,
    <T as FromStr>::Err: error::Error + Send,
{
    type Key = T;
    type Error = Error;

    type Unit = BoxFuture<(), Self::Error>;
    type Bool = BoxFuture<bool, Self::Error>;
    type Heads = BoxStream<Self::Key, Self::Error>;

    fn add(&self, key: &Self::Key) -> Self::Unit {
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
            .boxed()
    }

    fn remove(&self, key: &Self::Key) -> Self::Unit {
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
            .boxed()
    }

    fn is_head(&self, key: &Self::Key) -> Self::Bool {
        let pool = self.pool.clone();
        self.get_path(&key)
            .into_future()
            .and_then(move |path| {
                let future = poll_fn(move || Ok(Async::Ready(path.exists())));
                pool.spawn(future)
            })
            .boxed()
    }

    fn heads(&self) -> Self::Heads {
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
                        let name = T::from_str(name).chain_err(|| "can't parse name");
                        Some(name)
                    }
                    Ok(_) => None,
                    Err(err) => Some(Err(err)),
                })
        });
        match names {
            Ok(v) => stream::iter(v).boxed(),
            Err(e) => stream::once(Err(e.into())).boxed(),
        }
    }
}


#[cfg(test)]
mod test {
    use super::*;
    use futures::{Future, Stream};
    use mercurial_types::NodeHash;
    use mercurial_types::hash::Sha1;
    use std::str::FromStr;
    use tempdir::TempDir;

    #[test]
    fn basic() {
        let tmp = TempDir::new("filebookmarks_heads_basic").unwrap();
        let heads = FileHeads::open(tmp.path()).unwrap();
        let empty: Vec<String> = Vec::new();
        assert_eq!(heads.heads().collect().wait().unwrap(), empty);

        let foo = "foo".to_string();
        let bar = "bar".to_string();
        let baz = "baz".to_string();

        assert!(!heads.is_head(&foo).wait().unwrap());
        assert!(!heads.is_head(&bar).wait().unwrap());
        assert!(!heads.is_head(&baz).wait().unwrap());

        heads.add(&foo).wait().unwrap();
        heads.add(&bar).wait().unwrap();

        assert!(heads.is_head(&foo).wait().unwrap());
        assert!(heads.is_head(&bar).wait().unwrap());
        assert!(!heads.is_head(&baz).wait().unwrap());

        let mut result = heads.heads().collect().wait().unwrap();
        result.sort();

        assert_eq!(result, vec![bar.clone(), foo.clone()]);

        heads.remove(&foo).wait().unwrap();
        heads.remove(&bar).wait().unwrap();
        heads.remove(&baz).wait().unwrap(); // Removing non-existent head should not panic.

        assert_eq!(heads.heads().collect().wait().unwrap(), empty);
    }

    #[test]
    fn persistence() {
        let tmp = TempDir::new("filebookmarks_heads_persistence").unwrap();
        let foo = "foo".to_string();
        let bar = "bar".to_string();

        {
            let heads = FileHeads::open(tmp.path()).unwrap();
            heads.add(&foo).wait().unwrap();
            heads.add(&bar).wait().unwrap();
        }

        let heads = FileHeads::<String>::open(&tmp.path()).unwrap();
        let mut result = heads.heads().collect().wait().unwrap();
        result.sort();
        assert_eq!(result, vec![bar.clone(), foo.clone()]);
    }

    #[test]
    fn invalid_dir() {
        let tmp = TempDir::new("filebookmarks_heads_invalid_dir").unwrap();
        let heads = FileHeads::<String>::open(tmp.path().join("does_not_exist"));
        assert!(heads.is_err());
    }

    #[test]
    fn savenodehash() {
        let tmp = TempDir::new("filebookmarks_heads_nod").unwrap();
        {
            let h = (0..40).map(|_| "a").collect::<String>();
            let head = NodeHash::new(Sha1::from_str(h.as_str()).unwrap());
            let heads = FileHeads::<NodeHash>::open(tmp.path()).unwrap();
            heads.add(&head).wait().unwrap();
            let mut result = heads.heads().collect().wait().unwrap();
            result.sort();
            assert_eq!(result, vec![head]);
        }
    }
}
