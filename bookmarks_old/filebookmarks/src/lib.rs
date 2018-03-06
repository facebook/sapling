// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate bookmarks;

extern crate failure_ext as failure;
extern crate futures;
extern crate futures_cpupool;
extern crate percent_encoding;

extern crate filekv;
extern crate futures_ext;
extern crate mercurial_types;
extern crate storage_types;

use std::path::PathBuf;
use std::str;
use std::sync::Arc;

use failure::{Error, Result};
use futures::{Future, Stream};
use futures_cpupool::CpuPool;
use percent_encoding::{percent_decode, percent_encode, DEFAULT_ENCODE_SET};

use bookmarks::{Bookmarks, BookmarksMut};
use filekv::FileKV;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use mercurial_types::nodehash::ChangesetId;
use storage_types::Version;

static PREFIX: &'static str = "bookmark:";

/// A basic file-based persistent bookmark store.
///
/// Bookmarks are stored as files in the specified base directory. File operations are dispatched
/// to a thread pool to avoid blocking the main thread. File accesses between these threads
/// are synchronized by a global map of per-path locks.
pub struct FileBookmarks {
    kv: FileKV<ChangesetId>,
}

impl FileBookmarks {
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

impl Bookmarks for FileBookmarks {
    #[inline]
    fn get(&self, name: &AsRef<[u8]>) -> BoxFuture<Option<(ChangesetId, Version)>, Error> {
        self.kv
            .get(encode_key(name))
            .map_err(|e| e.context("FileBookmarks get failed").into())
            .boxify()
    }

    fn keys(&self) -> BoxStream<Vec<u8>, Error> {
        self.kv
            .keys()
            .and_then(|name| Ok(percent_decode(&name[..].as_bytes()).collect()))
            .map_err(|e| e.context("FileBookmarks keys failed").into())
            .boxify()
    }
}

impl BookmarksMut for FileBookmarks {
    #[inline]
    fn set(
        &self,
        key: &AsRef<[u8]>,
        value: &ChangesetId,
        version: &Version,
    ) -> BoxFuture<Option<Version>, Error> {
        self.kv
            .set(encode_key(key), value, version, None)
            .map_err(|e| e.context("FileBookmarks set failed").into())
            .boxify()
    }

    #[inline]
    fn delete(&self, key: &AsRef<[u8]>, version: &Version) -> BoxFuture<Option<Version>, Error> {
        self.kv
            .delete(encode_key(key), version)
            .map_err(|e| e.context("FileBookmarks delete failed").into())
            .boxify()
    }
}
