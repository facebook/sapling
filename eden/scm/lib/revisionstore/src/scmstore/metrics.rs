/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::AddAssign;

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

    // Total time spent completing the fetch
    time: usize,

    // Number of times data was computed/derved (i.e. aux data based on content).
    computed: usize,
}

impl AddAssign for FetchMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.requests += rhs.requests;
        self.keys += rhs.keys;
        self.hits += rhs.hits;
        self.misses += rhs.misses;
        self.errors += rhs.errors;
        self.time += rhs.time;
        self.computed += rhs.computed;
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

    pub(crate) fn computed(&mut self, keys: usize) {
        self.computed += keys;
    }

    // Provide the time as microseconds
    pub(crate) fn time(&mut self, keys: usize) {
        self.time += keys;
    }

    // Given a duration, perform a best effort conversion to microseconds and
    // record the value.
    pub(crate) fn time_from_duration(
        &mut self,
        keys: std::time::Duration,
    ) -> Result<(), anyhow::Error> {
        // We expect fetch times in microseconds to be << MAX_USIZE, so this
        // conversion should be safe.
        let usize: usize = keys.as_micros().try_into()?;
        self.time(usize);
        Ok(())
    }

    pub(crate) fn metrics(&self) -> impl Iterator<Item = (&'static str, usize)> {
        [
            ("requests", self.requests),
            ("keys", self.keys),
            ("hits", self.hits),
            ("misses", self.misses),
            ("errors", self.errors),
            ("time", self.time),
            ("computed", self.computed),
        ]
        .into_iter()
        .filter(|&(_, v)| v != 0)
    }
}

// TODO(meyer): I don't think this is in any critical paths, but it'd be nicer to rewrite this
// to use `Item = (Vec<&'static str>, usize)` instead of `Item = (String, usize)`, since all
// the fields are indeed statically named right now, or, better, just tree of some sort instead of a
// list of metrics. Probably appropriate for a `SmallVec` too, since the namespace depth is
// limited.
pub(crate) fn namespaced(
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreLocation {
    Local,
    Cache,
}

impl LocalAndCacheFetchMetrics {
    pub(crate) fn store(&mut self, loc: StoreLocation) -> &mut FetchMetrics {
        match loc {
            StoreLocation::Local => &mut self.local,
            StoreLocation::Cache => &mut self.cache,
        }
    }

    pub(crate) fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
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

    pub(crate) fn metrics(&self) -> impl Iterator<Item = (&'static str, usize)> {
        [("items", self.items), ("ok", self.ok), ("err", self.err)]
            .into_iter()
            .filter(|&(_, v)| v != 0)
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

    pub(crate) fn metrics(&self) -> impl Iterator<Item = (&'static str, usize)> {
        [
            ("calls", self.calls),
            ("keys", self.keys),
            ("singles", self.singles),
        ]
        .into_iter()
        .filter(|&(_, v)| v != 0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct CasBackendMetrics {
    /// Total number of bytes fetched from the CAS ZippyDb backend
    zdb_bytes: u64,

    /// Total number of queries to the CAS ZippyDb backend
    zdb_queries: u64,

    /// Total number of bytes fetched from the CAS ZGW backend
    zgw_bytes: u64,

    /// Total number of queries to the CAS ZGW backend
    zgw_queries: u64,

    /// Total number of bytes fetched from the CAS Manifold backend
    manifold_bytes: u64,

    /// Total number of queries to the CAS Manifold backend
    manifold_queries: u64,

    /// Total number of bytes fetched from the CAS Hedwig backend
    hedwig_bytes: u64,

    /// Total number of queries to the CAS Hedwig backend
    hedwig_queries: u64,
}

impl CasBackendMetrics {
    pub(crate) fn zdb_bytes(&mut self, bytes: u64) {
        self.zdb_bytes += bytes;
    }
    pub(crate) fn zdb_queries(&mut self, queries: u64) {
        self.zdb_queries += queries;
    }
    pub(crate) fn zgw_bytes(&mut self, bytes: u64) {
        self.zgw_bytes += bytes;
    }
    pub(crate) fn zgw_queries(&mut self, queries: u64) {
        self.zgw_queries += queries;
    }
    pub(crate) fn manifold_bytes(&mut self, bytes: u64) {
        self.manifold_bytes += bytes;
    }
    pub(crate) fn manifold_queries(&mut self, queries: u64) {
        self.manifold_queries += queries;
    }
    pub(crate) fn hedwig_bytes(&mut self, bytes: u64) {
        self.hedwig_bytes += bytes;
    }
    pub(crate) fn hedwig_queries(&mut self, queries: u64) {
        self.hedwig_queries += queries;
    }
    pub(crate) fn metrics(&self) -> impl Iterator<Item = (&'static str, usize)> {
        [
            ("zdb.bytes", self.zdb_bytes as usize),
            ("zgw.bytes", self.zgw_bytes as usize),
            ("manifold.bytes", self.manifold_bytes as usize),
            ("hedwig.bytes", self.hedwig_bytes as usize),
            ("zdb.queries", self.zdb_queries as usize),
            ("zgw.queries", self.zgw_queries as usize),
            ("manifold.queries", self.manifold_queries as usize),
            ("hedwig.queries", self.hedwig_queries as usize),
        ]
        .into_iter()
        .filter(|&(_, v)| v != 0)
    }
}

impl AddAssign for CasBackendMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.zdb_bytes += rhs.zdb_bytes;
        self.zgw_bytes += rhs.zgw_bytes;
        self.manifold_bytes += rhs.manifold_bytes;
        self.hedwig_bytes += rhs.hedwig_bytes;
        self.zdb_queries += rhs.zdb_queries;
        self.zgw_queries += rhs.zgw_queries;
        self.manifold_queries += rhs.manifold_queries;
        self.hedwig_queries += rhs.hedwig_queries;
    }
}
