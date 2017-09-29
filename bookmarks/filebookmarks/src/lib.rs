// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate bookmarks;

extern crate bincode;
#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate futures_cpupool;
extern crate nix;
extern crate percent_encoding;
extern crate rand;
extern crate serde;
#[cfg(test)]
extern crate tempdir;

extern crate futures_ext;

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, SeekFrom};
use std::io::prelude::*;
use std::marker::PhantomData;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::str;
use std::sync::{Arc, Mutex};

use bincode::{deserialize, serialize, Infinite};
use futures::{Async, Poll};
use futures::future::{poll_fn, Future, IntoFuture};
use futures::stream::{self, Stream};
use futures_cpupool::CpuPool;
use nix::fcntl::{self, FlockArg};
use nix::sys::stat;
use percent_encoding::{percent_decode, percent_encode, DEFAULT_ENCODE_SET};
use serde::Serialize;
use serde::de::DeserializeOwned;

use bookmarks::{Bookmarks, BookmarksMut, Version};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

mod errors {
    error_chain!{
        foreign_links {
            Bincode(::bincode::Error);
            De(::serde::de::value::Error);
            Io(::std::io::Error);
            Nix(::nix::Error);
        }
    }
}
pub use errors::*;

static PREFIX: &'static str = "bookmark:";

fn version_random() -> Version {
    Version::from(rand::random::<u64>())
}

/// A basic file-based persistent bookmark store.
///
/// Bookmarks are stored as files in the specified base directory. File operations are dispatched
/// to a thread pool to avoid blocking the main thread. File accesses between these threads
/// are synchronized by a global map of per-path locks.
pub struct FileBookmarks<V> {
    base: PathBuf,
    pool: Arc<CpuPool>,
    locks: Mutex<HashMap<String, Arc<Mutex<PathBuf>>>>,
    _marker: PhantomData<V>,
}

impl<V> FileBookmarks<V> {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_with_pool(path, Arc::new(CpuPool::new_num_cpus()))
    }

    pub fn open_with_pool<P: AsRef<Path>>(path: P, pool: Arc<CpuPool>) -> Result<Self> {
        if !path.as_ref().is_dir() {
            bail!("'{}' is not a directory", path.as_ref().to_string_lossy());
        }

        Ok(FileBookmarks {
            base: path.as_ref().to_path_buf(),
            pool: pool,
            locks: Mutex::new(HashMap::new()),
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

    /// Return a Mutex protecting the path to the file corresponding to the given key.
    /// Ensures that file accesses across multiple threads in the pool are syncrhonized.
    fn get_path_mutex(&self, key: &AsRef<[u8]>) -> Result<Arc<Mutex<PathBuf>>> {
        let key_string = percent_encode(key.as_ref(), DEFAULT_ENCODE_SET).to_string();
        let mut map = self.locks.lock().expect("Lock poisoned");
        let mutex = map.entry(key_string.clone()).or_insert_with(|| {
            let path = self.base.join(format!("{}{}", PREFIX, key_string));
            Arc::new(Mutex::new(path))
        });
        Ok((*mutex).clone())
    }
}

impl<V> Bookmarks for FileBookmarks<V>
where
    V: Clone + Serialize + DeserializeOwned + Send + 'static,
{
    type Value = V;
    type Error = Error;

    type Get = BoxFuture<Option<(Self::Value, Version)>, Self::Error>;
    type Keys = BoxStream<Vec<u8>, Self::Error>;

    fn get(&self, key: &AsRef<[u8]>) -> Self::Get {
        let pool = self.pool.clone();
        self.get_path_mutex(key)
            .into_future()
            .and_then(move |mutex| {
                let future = poll_fn(move || poll_get::<V>(&mutex));
                pool.spawn(future)
            })
            .boxify()
    }

    fn keys(&self) -> Self::Keys {
        // XXX: This traversal of the directory entries is unsynchronized and depends on
        // platform-specific behavior with respect to the underlying directory entries.
        // As a result, concurrent writes from other threads may produce strange results here.
        let names = fs::read_dir(&self.base).map(|entries| {
            entries
                .map(|result| {
                    result
                        .map_err(From::from)
                        .map(|entry| entry.file_name().to_string_lossy().into_owned())
                })
                .filter(|result| match result {
                    &Ok(ref name) => name.starts_with(PREFIX),
                    &Err(_) => true,
                })
                .map(|result| {
                    result.and_then(|name| {
                        Ok(percent_decode(&name[PREFIX.len()..].as_bytes()).collect())
                    })
                })
        });
        match names {
            Ok(v) => stream::iter_ok(v).and_then(|x| x).boxify(),
            Err(e) => stream::once(Err(e.into())).boxify(),
        }
    }
}

impl<V> BookmarksMut for FileBookmarks<V>
where
    V: Clone + Serialize + DeserializeOwned + Send + 'static,
{
    type Set = BoxFuture<Option<Version>, Self::Error>;

    fn set(&self, key: &AsRef<[u8]>, value: &Self::Value, version: &Version) -> Self::Set {
        let pool = self.pool.clone();
        let value = value.clone();
        let version = version.clone();
        self.get_path_mutex(key)
            .into_future()
            .and_then(move |mutex| {
                let future = poll_fn(move || poll_set(&mutex, &value, &version));
                pool.spawn(future)
            })
            .boxify()
    }

    fn delete(&self, key: &AsRef<[u8]>, version: &Version) -> Self::Set {
        let pool = self.pool.clone();
        let version = version.clone();
        self.get_path_mutex(key)
            .into_future()
            .and_then(move |mutex| {
                let future = poll_fn(move || poll_delete(&mutex, &version));
                pool.spawn(future)
            })
            .boxify()
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
                let new_version = version_random();
                let out = serialize(&(value, new_version), Infinite)?;
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
        let tmp = TempDir::new("filebookmarks_heads_basic").unwrap();
        let bookmarks = FileBookmarks::open(tmp.path()).unwrap();

        let foo = "foo".to_string();
        let one = "1".to_string();
        let two = "2".to_string();
        let three = "3".to_string();

        assert_eq!(bookmarks.get(&foo).wait().unwrap(), None);

        let absent = Version::absent();
        let foo_v1 = bookmarks.set(&foo, &one, &absent).wait().unwrap().unwrap();
        assert_eq!(
            bookmarks.get(&foo).wait().unwrap(),
            Some((one.clone(), foo_v1))
        );

        let foo_v2 = bookmarks.set(&foo, &two, &foo_v1).wait().unwrap().unwrap();

        // Should fail due to version mismatch.
        assert_eq!(bookmarks.set(&foo, &three, &foo_v1).wait().unwrap(), None);

        assert_eq!(
            bookmarks.delete(&foo, &foo_v2).wait().unwrap().unwrap(),
            absent
        );
        assert_eq!(bookmarks.get(&foo).wait().unwrap(), None);

        // Even though bookmark doesn't exist, this should fail with a version mismatch.
        assert_eq!(bookmarks.delete(&foo, &foo_v2).wait().unwrap(), None);

        // Deleting it with the absent version should work.
        assert_eq!(
            bookmarks.delete(&foo, &absent).wait().unwrap().unwrap(),
            absent
        );
    }

    #[test]
    fn persistence() {
        let tmp = TempDir::new("filebookmarks_heads_persistence").unwrap();
        let foo = "foo".to_string();
        let bar = "bar".to_string();

        let version;
        {
            let bookmarks = FileBookmarks::open(tmp.path()).unwrap();
            version = bookmarks.create(&foo, &bar).wait().unwrap().unwrap();
        }

        let bookmarks = FileBookmarks::open(tmp.path()).unwrap();
        assert_eq!(bookmarks.get(&foo).wait().unwrap(), Some((bar, version)));
    }

    #[test]
    fn list() {
        let tmp = TempDir::new("filebookmarks_heads_basic").unwrap();
        let bookmarks = FileBookmarks::open(tmp.path()).unwrap();

        let one = b"1";
        let two = b"2";
        let three = b"3";

        let _ = bookmarks
            .create(&one, &"foo".to_string())
            .wait()
            .unwrap()
            .unwrap();
        let _ = bookmarks
            .create(&two, &"bar".to_string())
            .wait()
            .unwrap()
            .unwrap();
        let _ = bookmarks
            .create(&three, &"baz".to_string())
            .wait()
            .unwrap()
            .unwrap();

        let mut result = bookmarks.keys().collect().wait().unwrap();
        result.sort();

        let expected = vec![one, two, three];
        assert_eq!(result, expected);
    }
}
