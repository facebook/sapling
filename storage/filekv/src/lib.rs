// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! A file-based key-value store. Uses `flock(2)` to guarantee cross-process consistency for reads
//! and writes.

#![deny(warnings)]

extern crate bincode;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_cpupool;
extern crate nix;
extern crate rand;
extern crate serde;
#[cfg(test)]
extern crate tempdir;

extern crate futures_ext;
extern crate storage_types;

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::{self, File, OpenOptions};
use std::io::{self, SeekFrom};
use std::io::prelude::*;
use std::marker::PhantomData;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bincode::{deserialize, serialize};
use futures::{Async, Poll};
use futures::future::{poll_fn, Future, IntoFuture};
use futures::stream::{self, Stream};
use futures_cpupool::CpuPool;
use nix::fcntl::{self, FlockArg};
use nix::sys::stat;
use serde::Serialize;
use serde::de::DeserializeOwned;

use futures_ext::{BoxStream, StreamExt};
use storage_types::{version_random, Version};

use failure::{Error, Result};

/// A basic file-based persistent bookmark store.
///
/// Key-value pairs are stored as files in the specified base directory. File operations are
/// dispatched to a thread pool to avoid blocking the main thread. File accesses between these
/// threads are synchronized by a global map of per-path locks.
pub struct FileKV<V> {
    base: PathBuf,
    prefix: String,
    pool: Arc<CpuPool>,
    locks: Mutex<HashMap<String, Arc<Mutex<PathBuf>>>>,
    _marker: PhantomData<V>,
}

impl<V> FileKV<V>
where
    V: Send + Clone + Serialize + DeserializeOwned + 'static,
{
    pub fn open<P, S>(path: P, prefix: S) -> Result<Self>
    where
        P: Into<PathBuf>,
        S: Into<String>,
    {
        Self::open_with_pool(path, prefix, Arc::new(CpuPool::new_num_cpus()))
    }

    pub fn open_with_pool<P, S>(path: P, prefix: S, pool: Arc<CpuPool>) -> Result<Self>
    where
        P: Into<PathBuf>,
        S: Into<String>,
    {
        let path = path.into();
        if !path.is_dir() {
            bail_msg!("'{}' is not a directory", path.to_string_lossy());
        }

        Ok(FileKV {
            base: path.into(),
            prefix: prefix.into(),
            pool: pool,
            locks: Mutex::new(HashMap::new()),
            _marker: PhantomData,
        })
    }

    pub fn create<P, S>(path: P, prefix: S) -> Result<Self>
    where
        P: Into<PathBuf>,
        S: Into<String>,
    {
        Self::create_with_pool(path, prefix, Arc::new(CpuPool::new_num_cpus()))
    }

    pub fn create_with_pool<P, S>(path: P, prefix: S, pool: Arc<CpuPool>) -> Result<Self>
    where
        P: Into<PathBuf>,
        S: Into<String>,
    {
        let path = path.into();
        fs::create_dir_all(&path)?;
        Self::open_with_pool(path, prefix, pool)
    }

    /// Return a Mutex protecting the path to the file corresponding to the given key.
    /// Ensures that file accesses across multiple threads in the pool are syncrhonized.
    fn get_path_mutex<Q: Into<String>>(&self, key: Q) -> Result<Arc<Mutex<PathBuf>>> {
        let mut map = self.locks.lock().expect("Lock poisoned");
        match map.entry(key.into()) {
            Entry::Occupied(occupied) => {
                let mutex = occupied.get();
                Ok((*mutex).clone())
            }
            Entry::Vacant(vacant) => {
                let path = self.base.join(format!("{}{}", self.prefix, vacant.key()));
                let mutex = vacant.insert(Arc::new(Mutex::new(path)));
                Ok((*mutex).clone())
            }
        }
    }

    pub fn get<Q: Into<String>>(
        &self,
        key: Q,
    ) -> impl Future<Item = Option<(V, Version)>, Error = Error> {
        let pool = self.pool.clone();
        self.get_path_mutex(key)
            .into_future()
            .and_then(move |mutex| {
                let future = poll_fn(move || poll_get::<V>(&mutex));
                pool.spawn(future)
            })
    }

    pub fn keys(&self) -> BoxStream<String, Error> {
        // XXX: This traversal of the directory entries is unsynchronized and depends on
        // platform-specific behavior with respect to the underlying directory entries.
        // As a result, concurrent writes from other threads may produce strange results here.

        let prefix = self.prefix.clone();
        let prefix_len = prefix.len();

        let names = fs::read_dir(&self.base).map(|entries| {
            entries
                .map(|result| {
                    result
                        .map_err(From::from)
                        .map(|entry| entry.file_name().to_string_lossy().into_owned())
                })
                .filter(move |result| match result {
                    &Ok(ref name) => name.starts_with(&prefix),
                    &Err(_) => true,
                })
                .map(move |result| result.and_then(|name| Ok(name[prefix_len..].into())))
        });
        match names {
            Ok(v) => stream::iter_ok(v).and_then(|x| x).boxify(),
            Err(e) => stream::once(Err(e.into())).boxify(),
        }
    }

    pub fn set<Q: Into<String>>(
        &self,
        key: Q,
        value: &V,
        version: &Version,
        new_version: Option<Version>,
    ) -> impl Future<Item = Option<Version>, Error = Error> {
        let pool = self.pool.clone();
        let value = value.clone();
        let version = version.clone();
        self.get_path_mutex(key)
            .into_future()
            .and_then(move |mutex| {
                let new_version = new_version.unwrap_or(version_random());
                let future = poll_fn(move || poll_set(&mutex, &value, &version, new_version));
                pool.spawn(future)
            })
    }

    // Convenience function for creating new keys (since initial version is always "absent").
    #[inline]
    pub fn set_new<Q: Into<String>>(
        &self,
        key: Q,
        value: &V,
        new_version: Option<Version>,
    ) -> impl Future<Item = Option<Version>, Error = Error> {
        self.set(key, value, &Version::absent(), new_version)
    }

    pub fn delete<Q: Into<String>>(
        &self,
        key: Q,
        version: &Version,
    ) -> impl Future<Item = Option<Version>, Error = Error> {
        let pool = self.pool.clone();
        let version = version.clone();
        self.get_path_mutex(key)
            .into_future()
            .and_then(move |mutex| {
                let future = poll_fn(move || poll_delete(&mutex, &version));
                pool.spawn(future)
            })
    }
}

/// Synchronous implementation of the get operation for the bookmark store. Intended to
/// be used in conjunction with poll_fn() and a CpuPool to dispatch it onto a thread pool.
fn poll_get<V>(path_mutex: &Arc<Mutex<PathBuf>>) -> Poll<Option<(V, Version)>, Error>
where
    V: DeserializeOwned,
{
    let path = path_mutex.lock().expect("Lock poisoned");

    let result = match File::open(&*path) {
        Ok(mut file) => {
            // Block until we get an advisory lock on this file.
            let fd = file.as_raw_fd();
            fcntl::flock(fd, FlockArg::LockShared)?;

            // Ensure file wasn't deleted between opening and locking.
            if stat::fstat(fd)?.st_nlink > 0 {
                let mut buf = Vec::new();
                let _ = file.read_to_end(&mut buf)?;
                Ok(Some(deserialize(&buf)?))
            } else {
                Ok(None)
            }
        }
        Err(e) => {
            // Return None instead of an Error if the file doesn't exist.
            match e.kind() {
                io::ErrorKind::NotFound => Ok(None),
                _ => Err(e.into()),
            }
        }
    };

    result.map(Async::Ready)
}

/// Synchronous implementation of the set operation for the bookmark store. Intended to
/// be used in conjunction with poll_fn() and a CpuPool to dispatch it onto a thread pool.
fn poll_set<V>(
    path_mutex: &Arc<Mutex<PathBuf>>,
    value: &V,
    version: &Version,
    new_version: Version,
) -> Poll<Option<Version>, Error>
where
    V: Serialize,
{
    let path = path_mutex.lock().expect("Lock poisoned");
    let mut options = OpenOptions::new();
    options.read(true).write(true);

    // If we expect the file to not exist, disallow opening an existing file.
    if *version == Version::absent() {
        options.create_new(true);
    }

    let result = match options.open(&*path) {
        Ok(mut file) => {
            // Block until we get an advisory lock on this file.
            let fd = file.as_raw_fd();
            fcntl::flock(fd, FlockArg::LockExclusive)?;

            // Read version.
            let file_version = if *version == Version::absent() {
                Version::absent()
            } else {
                let mut buf = Vec::new();
                let _ = file.read_to_end(&mut buf)?;
                deserialize::<(String, Version)>(&buf)?.1
            };

            // Write out new value if versions match.
            if file_version == *version {
                let out = serialize(&(value, new_version))?;
                file.seek(SeekFrom::Start(0))?;
                file.set_len(0)?;
                file.write_all(&out)?;
                Ok(Some(new_version))
            } else {
                Ok(None)
            }
        }
        Err(e) => {
            // We can only get EEXIST if the version was specified as absent but
            // the file exists. This is a version mismatch, so return None accordingly.
            match e.kind() {
                io::ErrorKind::AlreadyExists => Ok(None),
                _ => Err(e.into()),
            }
        }
    };

    result.map(Async::Ready)
}

/// Synchronous implementation of the delete operation for the bookmark store. Intended to
/// be used in conjunction with poll_fn() and a CpuPool to dispatch it onto a thread pool.
fn poll_delete(
    path_mutex: &Arc<Mutex<PathBuf>>,
    version: &Version,
) -> Poll<Option<Version>, Error> {
    let path = path_mutex.lock().expect("Lock poisoned");

    let result = match File::open(&*path) {
        Ok(mut file) => {
            // Block until we get an advisory lock on this file.
            let fd = file.as_raw_fd();
            fcntl::flock(fd, FlockArg::LockExclusive)?;

            // Read version.
            let mut buf = Vec::new();
            let _ = file.read_to_end(&mut buf)?;
            let file_version = deserialize::<(String, Version)>(&buf)?.1;

            // Unlink files if version matches, reporting success if the file
            // has already been deleted by another thread or process.
            if file_version == *version {
                fs::remove_file(&*path).or_else(|e| match e.kind() {
                    io::ErrorKind::NotFound => Ok(()),
                    _ => Err(e),
                })?;
                Ok(Some(Version::absent()))
            } else {
                Ok(None)
            }
        }
        Err(e) => {
            // Check for absent version if the file doesn't exist.
            match e.kind() {
                io::ErrorKind::NotFound => {
                    if *version == Version::absent() {
                        // Report successful deletion of non-existent bookmark.
                        Ok(Some(Version::absent()))
                    } else {
                        // Version mismatch.
                        Ok(None)
                    }
                }
                _ => Err(e.into()),
            }
        }
    };

    result.map(Async::Ready)
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::{Future, Stream};
    use tempdir::TempDir;

    #[test]
    fn basic() {
        let tmp = TempDir::new("filekv_basic").unwrap();
        let kv = FileKV::open(tmp.path(), "kv:").unwrap();

        let foo = "foo";
        let one = "1".to_string();
        let two = "2".to_string();
        let three = "3".to_string();

        assert_eq!(kv.get(foo).wait().unwrap(), None);

        let absent = Version::absent();
        let foo_v1 = kv.set(foo, &one, &absent, None).wait().unwrap().unwrap();
        assert_eq!(kv.get(foo).wait().unwrap(), Some((one.clone(), foo_v1)));

        let foo_v2 = kv.set(foo, &two, &foo_v1, None).wait().unwrap().unwrap();

        // Should fail due to version mismatch.
        assert_eq!(kv.set(foo, &three, &foo_v1, None).wait().unwrap(), None);

        assert_eq!(kv.delete(foo, &foo_v2).wait().unwrap().unwrap(), absent);
        assert_eq!(kv.get(foo).wait().unwrap(), None);

        // Even though bookmark doesn't exist, this should fail with a version mismatch.
        assert_eq!(kv.delete(foo, &foo_v2).wait().unwrap(), None);

        // Deleting it with the absent version should work.
        assert_eq!(kv.delete(foo, &absent).wait().unwrap().unwrap(), absent);
    }

    #[test]
    fn persistence() {
        let tmp = TempDir::new("filebookmarks_heads_persistence").unwrap();
        let foo = "foo";
        let bar = "bar".to_string();

        let version;
        {
            let kv = FileKV::open(tmp.path(), "kv:").unwrap();
            version = kv.set_new(foo, &bar, None).wait().unwrap().unwrap();
        }

        let kv = FileKV::open(tmp.path(), "kv:").unwrap();
        assert_eq!(kv.get(foo).wait().unwrap(), Some((bar, version)));
    }

    #[test]
    fn list() {
        let tmp = TempDir::new("filebookmarks_heads_basic").unwrap();
        let kv = FileKV::open(tmp.path(), "kv:").unwrap();

        let one = "1";
        let two = "2";
        let three = "3";

        let _ = kv.set_new(one, &"foo".to_string(), None)
            .wait()
            .unwrap()
            .unwrap();
        let _ = kv.set_new(two, &"bar".to_string(), None)
            .wait()
            .unwrap()
            .unwrap();
        let _ = kv.set_new(three, &"baz".to_string(), None)
            .wait()
            .unwrap()
            .unwrap();

        let mut result = kv.keys().collect().wait().unwrap();
        result.sort();

        let expected = vec![one, two, three];
        assert_eq!(result, expected);
    }
}
