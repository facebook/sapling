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

extern crate filekv;
extern crate futures_ext;
extern crate storage_types;

use std::path::PathBuf;
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
    pub fn open<P: Into<PathBuf>>(path: P) -> Result<Self> {
        Ok(FileBookmarks {
            kv: FileKV::open(path, PREFIX)?,
        })
    }

    #[inline]
    pub fn open_with_pool<P: Into<PathBuf>>(path: P, pool: Arc<CpuPool>) -> Result<Self> {
        Ok(FileBookmarks {
            kv: FileKV::open_with_pool(path, PREFIX, pool)?,
        })
    }

    #[inline]
    pub fn create<P: Into<PathBuf>>(path: P) -> Result<Self> {
        Ok(FileBookmarks {
            kv: FileKV::create(path, PREFIX)?,
        })
    }

    #[inline]
    pub fn create_with_pool<P: Into<PathBuf>>(path: P, pool: Arc<CpuPool>) -> Result<Self> {
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
