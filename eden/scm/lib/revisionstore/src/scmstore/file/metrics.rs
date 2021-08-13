/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::AddAssign;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use tracing::instrument;

use types::Key;

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

#[derive(Debug, Default, Clone)]
pub struct FileStoreMetrics {
    pub(crate) fetch: FileStoreFetchMetrics,
}

impl FileStoreMetrics {
    pub fn new() -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(FileStoreMetrics::default()))
    }

    pub fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
        namespaced("scmstore.file.fetch", self.fetch.metrics())
    }
}

#[derive(Debug, Default)]
pub(crate) struct ContentStoreFallbacksInner {
    write_ptr: u64,
}

#[derive(Debug)]
pub struct ContentStoreFallbacks {
    inner: Mutex<ContentStoreFallbacksInner>,
}

impl ContentStoreFallbacks {
    pub(crate) fn new() -> Self {
        ContentStoreFallbacks {
            inner: Mutex::new(ContentStoreFallbacksInner::default()),
        }
    }

    #[instrument(level = "warn", skip(self))]
    pub(crate) fn write_ptr(&self, _key: &Key) {
        self.inner.lock().write_ptr += 1;
    }

    pub fn write_ptr_count(&self) -> u64 {
        self.inner.lock().write_ptr
    }
}
