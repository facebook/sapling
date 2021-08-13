/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::AddAssign;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::indexedlogutil::StoreType;

#[derive(Clone, Debug, Default)]
pub struct FetchMetrics {
    /// Number of requests / batches
    requests: usize,

    /// Numbers of entities requested
    keys: usize,

    /// Number of successfully fetched entities
    hits: usize,

    /// Number of entities which were not found
    misses: usize,

    /// Number of entities which returned a fetch error (including batch errors)
    errors: usize,
}

impl AddAssign for FetchMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.requests += rhs.requests;
        self.keys += rhs.keys;
        self.hits += rhs.hits;
        self.misses += rhs.misses;
        self.errors += rhs.errors;
    }
}

impl FetchMetrics {
    pub(crate) fn fetch(&mut self, keys: usize) {
        self.requests += 1;
        self.keys += keys;
    }

    pub(crate) fn hit(&mut self, keys: usize) {
        self.hits += keys;
    }

    pub(crate) fn miss(&mut self, keys: usize) {
        self.misses += keys;
    }

    pub(crate) fn err(&mut self, keys: usize) {
        self.errors += keys;
    }

    fn metrics(&self) -> impl Iterator<Item = (&'static str, usize)> {
        std::array::IntoIter::new([
            ("requests", self.requests),
            ("keys", self.keys),
            ("hits", self.hits),
            ("misses", self.misses),
            ("errors", self.errors),
        ])
        .filter(|&(_, v)| v != 0)
    }
}

// TODO(meyer): I don't think this is in any critical paths, but it'd be nicer to rewrite this
// to use `Item = (Vec<&'static str>, usize)` instead of `Item = (String, usize)`, since all
// the fields are indeed statically named right now, or, better, just tree of some sort instead of a
// list of metrics. Probably appropriate for a `SmallVec` too, since the namespace depth is
// limited.
fn namespaced(
    namespace: &'static str,
    metrics: impl Iterator<Item = (impl AsRef<str>, usize)>,
) -> impl Iterator<Item = (String, usize)> {
    metrics.map(move |(k, v)| (namespace.to_string() + "." + k.as_ref(), v))
}

#[derive(Clone, Debug, Default)]
pub struct LocalAndCacheFetchMetrics {
    local: FetchMetrics,
    cache: FetchMetrics,
}

impl LocalAndCacheFetchMetrics {
    pub(crate) fn store(&mut self, typ: StoreType) -> &mut FetchMetrics {
        match typ {
            StoreType::Local => &mut self.local,
            StoreType::Shared => &mut self.cache,
        }
    }

    fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
        namespaced("local", self.local.metrics()).chain(namespaced("cache", self.cache.metrics()))
    }
}

impl AddAssign for LocalAndCacheFetchMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.local += rhs.local;
        self.cache += rhs.cache;
    }
}

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
pub struct WriteMetrics {
    /// Numbers of entities we attempted to write
    items: usize,

    /// Number of successfully written entities
    ok: usize,

    /// Number of entities which returned a write error (including batch errors)
    err: usize,
}

impl AddAssign for WriteMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.items += rhs.items;
        self.ok += rhs.ok;
        self.err += rhs.err;
    }
}

impl WriteMetrics {
    pub(crate) fn item(&mut self, keys: usize) {
        self.items += keys;
    }

    pub(crate) fn ok(&mut self, keys: usize) {
        self.ok += keys;
    }

    pub(crate) fn err(&mut self, keys: usize) {
        self.err += keys;
    }

    fn metrics(&self) -> impl Iterator<Item = (&'static str, usize)> {
        std::array::IntoIter::new([("items", self.items), ("ok", self.ok), ("err", self.err)])
            .filter(|&(_, v)| v != 0)
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
pub struct ApiMetrics {
    /// Number of calls to this API
    calls: usize,

    /// Total number of entities requested across all calls
    keys: usize,

    /// Number of calls for only a single entity
    singles: usize,
}

impl AddAssign for ApiMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.calls += rhs.calls;
        self.keys += rhs.keys;
        self.singles += rhs.singles;
    }
}

impl ApiMetrics {
    pub(crate) fn call(&mut self, keys: usize) {
        self.calls += 1;
        self.keys += keys;
        if keys == 1 {
            self.singles += 1;
        }
    }

    fn metrics(&self) -> impl Iterator<Item = (&'static str, usize)> {
        std::array::IntoIter::new([
            ("calls", self.calls),
            ("keys", self.keys),
            ("singles", self.singles),
        ])
        .filter(|&(_, v)| v != 0)
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
