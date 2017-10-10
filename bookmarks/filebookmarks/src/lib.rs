// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate bookmarks;

#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate futures_cpupool;
extern crate percent_encoding;
extern crate serde;
#[cfg(test)]
extern crate tempdir;

extern crate filekv;
extern crate futures_ext;
extern crate storage_types;

use std::path::Path;
use std::str;
use std::sync::Arc;

use futures::{Future, Stream};
use futures_cpupool::CpuPool;
use percent_encoding::{percent_decode, percent_encode, DEFAULT_ENCODE_SET};
use serde::Serialize;
use serde::de::DeserializeOwned;

use bookmarks::{Bookmarks, BookmarksMut};
use filekv::FileKV;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use storage_types::Version;

mod errors {
    error_chain! {
        links {
            FileKV(::filekv::Error, ::filekv::ErrorKind);
        }
    }
}
pub use errors::*;

static PREFIX: &'static str = "bookmark:";

/// A basic file-based persistent bookmark store.
///
/// Bookmarks are stored as files in the specified base directory. File operations are dispatched
/// to a thread pool to avoid blocking the main thread. File accesses between these threads
/// are synchronized by a global map of per-path locks.
pub struct FileBookmarks<V> {
    kv: FileKV<V>,
}

impl<V> FileBookmarks<V>
where
    V: Send + Clone + Serialize + DeserializeOwned + 'static,
{
    #[inline]
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(FileBookmarks {
            kv: FileKV::open(path, PREFIX)?,
        })
    }

    #[inline]
    pub fn open_with_pool<P: AsRef<Path>>(path: P, pool: Arc<CpuPool>) -> Result<Self> {
        Ok(FileBookmarks {
            kv: FileKV::open_with_pool(path, PREFIX, pool)?,
        })
    }

    #[inline]
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(FileBookmarks {
            kv: FileKV::create(path, PREFIX)?,
        })
    }

    #[inline]
    pub fn create_with_pool<P: AsRef<Path>>(path: P, pool: Arc<CpuPool>) -> Result<Self> {
        Ok(FileBookmarks {
            kv: FileKV::create_with_pool(path, PREFIX, pool)?,
        })
    }
}

#[inline]
fn encode_key(key: &AsRef<[u8]>) -> String {
    percent_encode(key.as_ref(), DEFAULT_ENCODE_SET).to_string()
}

impl<V> Bookmarks for FileBookmarks<V>
where
    V: Clone + Serialize + DeserializeOwned + Send + 'static,
{
    type Value = V;
    type Error = Error;

    type Get = BoxFuture<Option<(Self::Value, Version)>, Self::Error>;
    type Keys = BoxStream<Vec<u8>, Self::Error>;

    #[inline]
    fn get(&self, key: &AsRef<[u8]>) -> Self::Get {
        self.kv.get(encode_key(key)).from_err().boxify()
    }

    fn keys(&self) -> Self::Keys {
        self.kv
            .keys()
            .and_then(|name| Ok(percent_decode(&name[..].as_bytes()).collect()))
            .from_err()
            .boxify()
    }
}

impl<V> BookmarksMut for FileBookmarks<V>
where
    V: Clone + Serialize + DeserializeOwned + Send + 'static,
{
    type Set = BoxFuture<Option<Version>, Self::Error>;

    #[inline]
    fn set(&self, key: &AsRef<[u8]>, value: &Self::Value, version: &Version) -> Self::Set {
        self.kv
            .set(encode_key(key), value, version)
            .from_err()
            .boxify()
    }

    #[inline]
    fn delete(&self, key: &AsRef<[u8]>, version: &Version) -> Self::Set {
        self.kv.delete(encode_key(key), version).from_err().boxify()
    }
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
