/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::AddAssign;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::scmstore::metrics::namespaced;
use crate::scmstore::metrics::ApiMetrics;
use crate::scmstore::metrics::FetchMetrics;
use crate::scmstore::metrics::LocalAndCacheFetchMetrics;
use crate::scmstore::metrics::WriteMetrics;

#[derive(Clone, Debug, Default)]
pub struct ContentStoreFetchMetrics {
    /// Only content hits are counted by common.hits, LFS pointer hits are only counted below.
    common: FetchMetrics,

    /// ContentStore returned a serialized LFS pointer instead of file content.
    lfsptr_hits: usize,
}

impl ContentStoreFetchMetrics {
    pub(crate) fn fetch(&mut self, keys: usize) {
        self.common.fetch(keys)
    }

    pub(crate) fn hit(&mut self, keys: usize) {
        self.common.hit(keys)
    }

    pub(crate) fn miss(&mut self, keys: usize) {
        self.common.miss(keys)
    }

    pub(crate) fn err(&mut self, keys: usize) {
        self.common.err(keys)
    }

    pub(crate) fn hit_lfsptr(&mut self, keys: usize) {
        self.lfsptr_hits += keys;
    }

    fn metrics(&self) -> impl Iterator<Item = (&'static str, usize)> {
        std::iter::once(("lfsptrhits", self.lfsptr_hits))
            .filter(|&(_, v)| v != 0)
            .chain(self.common.metrics())
    }
}

impl AddAssign for ContentStoreFetchMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.common += rhs.common;
        self.lfsptr_hits += rhs.lfsptr_hits;
    }
}

#[derive(Clone, Debug, Default)]
pub struct FileStoreFetchMetrics {
    pub(crate) indexedlog: LocalAndCacheFetchMetrics,
    pub(crate) lfs: LocalAndCacheFetchMetrics,
    pub(crate) aux: LocalAndCacheFetchMetrics,
    pub(crate) contentstore: ContentStoreFetchMetrics,
}

impl AddAssign for FileStoreFetchMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.indexedlog += rhs.indexedlog;
        self.lfs += rhs.lfs;
        self.aux += rhs.aux;
        self.contentstore += rhs.contentstore;
    }
}

impl FileStoreFetchMetrics {
    fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
        namespaced("indexedlog", self.indexedlog.metrics())
            .chain(namespaced("lfs", self.lfs.metrics()))
            .chain(namespaced("aux", self.aux.metrics()))
            .chain(namespaced("contentstore", self.contentstore.metrics()))
    }
}

#[derive(Clone, Debug, Default)]
pub struct FileStoreWriteMetrics {
    /// Writes to the local LFS backend
    pub(crate) lfs: WriteMetrics,

    /// Writes to the local non-LFS backend
    pub(crate) nonlfs: WriteMetrics,

    /// LFS Pointer-only writes (supported only through fallback)
    pub(crate) lfsptr: WriteMetrics,
}

impl AddAssign for FileStoreWriteMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.lfs += rhs.lfs;
        self.nonlfs += rhs.nonlfs;
        self.lfsptr += rhs.lfsptr;
    }
}

impl FileStoreWriteMetrics {
    fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
        namespaced("lfs", self.lfs.metrics())
            .chain(namespaced("nonlfs", self.nonlfs.metrics()))
            .chain(namespaced("lfsptr", self.lfsptr.metrics()))
    }
}

#[derive(Clone, Debug, Default)]
pub struct FileStoreApiMetrics {
    pub(crate) hg_getfilecontent: ApiMetrics,
    pub(crate) hg_addpending: ApiMetrics,
    pub(crate) hg_commitpending: ApiMetrics,
    pub(crate) hg_get: ApiMetrics,
    pub(crate) hg_getmeta: ApiMetrics,
    pub(crate) hg_refresh: ApiMetrics,
    pub(crate) hg_prefetch: ApiMetrics,
    pub(crate) hg_upload: ApiMetrics,
    pub(crate) hg_getmissing: ApiMetrics,
    pub(crate) hg_add: ApiMetrics,
    pub(crate) hg_flush: ApiMetrics,
    pub(crate) contentdatastore_blob: ApiMetrics,
    pub(crate) contentdatastore_metadata: ApiMetrics,
}

impl AddAssign for FileStoreApiMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.hg_getfilecontent += rhs.hg_getfilecontent;
        self.hg_addpending += rhs.hg_addpending;
        self.hg_commitpending += rhs.hg_commitpending;
        self.hg_get += rhs.hg_get;
        self.hg_getmeta += rhs.hg_getmeta;
        self.hg_refresh += rhs.hg_refresh;
        self.hg_prefetch += rhs.hg_prefetch;
        self.hg_upload += rhs.hg_upload;
        self.hg_getmissing += rhs.hg_getmissing;
        self.hg_add += rhs.hg_add;
        self.hg_flush += rhs.hg_flush;
        self.contentdatastore_blob += rhs.contentdatastore_blob;
        self.contentdatastore_metadata += rhs.contentdatastore_metadata;
    }
}

impl FileStoreApiMetrics {
    fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
        namespaced("hg_getfilecontent", self.hg_getfilecontent.metrics())
            .chain(namespaced("hg_addpending", self.hg_addpending.metrics()))
            .chain(namespaced(
                "hg_commitpending",
                self.hg_commitpending.metrics(),
            ))
            .chain(namespaced("hg_get", self.hg_get.metrics()))
            .chain(namespaced("hg_getmeta", self.hg_getmeta.metrics()))
            .chain(namespaced("hg_refresh", self.hg_refresh.metrics()))
            .chain(namespaced("hg_prefetch", self.hg_prefetch.metrics()))
            .chain(namespaced("hg_upload", self.hg_upload.metrics()))
            .chain(namespaced("hg_getmissing", self.hg_getmissing.metrics()))
            .chain(namespaced("hg_add", self.hg_add.metrics()))
            .chain(namespaced(
                "contentdatastore_blob",
                self.contentdatastore_blob.metrics(),
            ))
            .chain(namespaced(
                "contentdatastore_metadata",
                self.contentdatastore_metadata.metrics(),
            ))
    }
}

#[derive(Debug, Default, Clone)]
pub struct FileStoreMetrics {
    pub(crate) fetch: FileStoreFetchMetrics,
    pub(crate) write: FileStoreWriteMetrics,
    pub(crate) api: FileStoreApiMetrics,
}

impl FileStoreMetrics {
    pub fn new() -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(FileStoreMetrics::default()))
    }

    pub fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
        namespaced(
            "scmstore.file",
            namespaced("fetch", self.fetch.metrics())
                .chain(namespaced("write", self.write.metrics()))
                .chain(namespaced("api", self.api.metrics())),
        )
    }
}
