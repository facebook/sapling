/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ops::AddAssign;

use ::metrics::Counter;
use types::CasFetchedStats;

pub struct FetchMetrics {
    /// Number of requests / batches
    pub(crate) requests: &'static Counter,

    /// Number of entities requested unbatched (i.e. not part of a batch)
    pub(crate) singles: &'static Counter,

    /// Numbers of entities requested
    pub(crate) keys: &'static Counter,

    /// Number of successfully fetched entities
    pub(crate) hits: &'static Counter,

    /// Number of entities which were not found
    pub(crate) misses: &'static Counter,

    /// Number of entities which returned a fetch error (including batch errors)
    pub(crate) errors: &'static Counter,

    // Total time spent completing the fetch
    pub(crate) time: &'static Counter,

    // Number of times data was computed/derived (i.e. aux data based on content).
    pub(crate) computed: &'static Counter,
}

/// Define a static Counter for FetchMetrics fields, and then construct a static FetchMetrics instance.
macro_rules! static_fetch_metrics {
    ($name:ident, $prefix:expr) => {
        paste::paste! {
            mod [<fetch_metrics_ $name:lower>] {
                pub static REQUESTS: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".requests"));
                pub static SINGLES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".singles"));
                pub static KEYS: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".keys"));
                pub static HITS: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".hits"));
                pub static MISSES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".misses"));
                pub static ERRORS: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".errors"));
                pub static TIME: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".time"));
                pub static COMPUTED: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".computed"));
            }

            static $name: $crate::scmstore::metrics::FetchMetrics = $crate::scmstore::metrics::FetchMetrics {
                requests: &[<fetch_metrics_ $name:lower>]::REQUESTS,
                singles: &[<fetch_metrics_ $name:lower>]::SINGLES,
                keys: &[<fetch_metrics_ $name:lower>]::KEYS,
                hits: &[<fetch_metrics_ $name:lower>]::HITS,
                misses: &[<fetch_metrics_ $name:lower>]::MISSES,
                errors: &[<fetch_metrics_ $name:lower>]::ERRORS,
                time: &[<fetch_metrics_ $name:lower>]::TIME,
                computed: &[<fetch_metrics_ $name:lower>]::COMPUTED,
            };
        }
    };
}

pub(crate) use static_fetch_metrics;

/// Construct a static LocalAndCacheFetchMetrics instance.
macro_rules! static_local_cache_fetch_metrics {
    ($name:ident, $prefix:tt) => {
        paste::paste! {
            $crate::scmstore::metrics::static_fetch_metrics!([<FETCH_METRICS_ $name _LOCAL>], concat!($prefix, ".local"));
            $crate::scmstore::metrics::static_fetch_metrics!([<FETCH_METRICS_ $name _CACHE>], concat!($prefix, ".cache"));

            static $name: $crate::scmstore::metrics::LocalAndCacheFetchMetrics = $crate::scmstore::metrics::LocalAndCacheFetchMetrics {
                local: &[<FETCH_METRICS_ $name _LOCAL>],
                cache: &[<FETCH_METRICS_ $name _CACHE>],
            };
        }
    }
}

pub(crate) use static_local_cache_fetch_metrics;

impl FetchMetrics {
    pub(crate) fn fetch(&self, keys: usize) {
        self.requests.increment();
        if keys == 1 {
            self.singles.increment();
        }
        self.keys.add(keys);
    }

    pub(crate) fn hit(&self, keys: usize) {
        self.hits.add(keys);
    }

    pub(crate) fn miss(&self, keys: usize) {
        self.misses.add(keys);
    }

    pub(crate) fn err(&self, keys: usize) {
        self.errors.add(keys);
    }

    pub(crate) fn computed(&self, keys: usize) {
        self.computed.add(keys);
    }

    // Provide the time as microseconds
    pub(crate) fn time(&self, keys: usize) {
        self.time.add(keys);
    }

    // Given a duration, perform a best effort conversion to microseconds and
    // record the value.
    pub(crate) fn time_from_duration(
        &self,
        keys: std::time::Duration,
    ) -> Result<(), anyhow::Error> {
        // We expect fetch times in microseconds to be << MAX_USIZE, so this
        // conversion should be safe.
        let usize: usize = keys.as_micros().try_into()?;
        self.time(usize);
        Ok(())
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

#[derive(Clone)]
pub struct LocalAndCacheFetchMetrics {
    pub(crate) local: &'static FetchMetrics,
    pub(crate) cache: &'static FetchMetrics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreLocation {
    Local,
    Cache,
}

impl LocalAndCacheFetchMetrics {
    pub(crate) fn store(&self, loc: StoreLocation) -> &'static FetchMetrics {
        match loc {
            StoreLocation::Local => self.local,
            StoreLocation::Cache => self.cache,
        }
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

    pub(crate) fn metrics(&self) -> impl Iterator<Item = (&'static str, usize)> + use<> {
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

    pub(crate) fn metrics(&self) -> impl Iterator<Item = (&'static str, usize)> + use<> {
        [
            ("calls", self.calls),
            ("keys", self.keys),
            ("singles", self.singles),
        ]
        .into_iter()
        .filter(|&(_, v)| v != 0)
    }
}

pub struct CasBackendMetrics {
    /// Total number of bytes fetched from the CAS ZippyDb backend
    pub(crate) zdb_bytes: &'static Counter,

    /// Total number of queries to the CAS ZippyDb backend
    pub(crate) zdb_queries: &'static Counter,

    /// Total number of bytes fetched from the CAS ZGW backend
    pub(crate) zgw_bytes: &'static Counter,

    /// Total number of queries to the CAS ZGW backend
    pub(crate) zgw_queries: &'static Counter,

    /// Total number of bytes fetched from the CAS Manifold backend
    pub(crate) manifold_bytes: &'static Counter,

    /// Total number of queries to the CAS Manifold backend
    pub(crate) manifold_queries: &'static Counter,

    /// Total number of bytes fetched from the CAS Hedwig backend
    pub(crate) hedwig_bytes: &'static Counter,

    /// Total number of queries to the CAS Hedwig backend
    pub(crate) hedwig_queries: &'static Counter,
}

pub struct CasLocalCacheMetrics {
    /// Total number of files fetched from the CAS Local Cache
    pub(crate) local_cache_hits_files: &'static Counter,

    /// Total number of bytes fetched from the CAS Local Cache
    pub(crate) local_cache_hits_bytes: &'static Counter,

    /// Total number of files not found in the CAS Local Cache
    pub(crate) local_cache_misses_files: &'static Counter,

    /// Total number of bytes not found in the CAS Local Cache
    pub(crate) local_cache_misses_bytes: &'static Counter,

    /// Total number of blobs fetched from the CAS Lmdb Local Cache
    pub(crate) local_lmdb_cache_hits_blobs: &'static Counter,

    /// Total number of bytes fetched from the CAS Lmdb Local Cache
    pub(crate) local_lmdb_cache_hits_bytes: &'static Counter,

    /// Total number of blobs missed from the CAS Lmdb Local Cache
    /// /// These only count for blobs that could be potentially fetched from LMDB based on the
    /// digest size
    pub(crate) local_lmdb_cache_misses_blobs: &'static Counter,

    /// Total number of bytes missed from the CAS Lmdb Local Cache
    /// /// /// These only count for bytes that could be potentially fetched from LMDB based on the
    /// digest size
    pub(crate) local_lmdb_cache_misses_bytes: &'static Counter,

    /// number of blooms not present in cloom
    pub(crate) local_cloom_misses: &'static Counter,

    /// number of blooms present in cloom
    pub(crate) local_cloom_true_positives: &'static Counter,

    /// number of blooms that cloom says are present, but are not
    pub(crate) local_cloom_false_positives: &'static Counter,
}

macro_rules! static_cas_backend_metrics {
    ($name:ident, $prefix:tt) => {
        paste::paste! {
            mod [<cas_metrics_ $name:lower>] {
                pub static ZDB_BYTES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".zdb.bytes"));
                pub static ZDB_QUERIES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".zdb.queries"));
                pub static ZGW_BYTES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".zgw.bytes"));
                pub static ZGW_QUERIES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".zgw.queries"));
                pub static MANIFOLD_BYTES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".manifold.bytes"));
                pub static MANIFOLD_QUERIES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".manifold.queries"));
                pub static HEDWIG_BYTES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".hedwig.bytes"));
                pub static HEDWIG_QUERIES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".hedwig.queries"));
            }

            static $name: $crate::scmstore::metrics::CasBackendMetrics = $crate::scmstore::metrics::CasBackendMetrics {
                zdb_bytes: &[<cas_metrics_ $name:lower>]::ZDB_BYTES,
                zdb_queries: &[<cas_metrics_ $name:lower>]::ZDB_QUERIES,
                zgw_bytes: &[<cas_metrics_ $name:lower>]::ZGW_BYTES,
                zgw_queries: &[<cas_metrics_ $name:lower>]::ZGW_QUERIES,
                manifold_bytes: &[<cas_metrics_ $name:lower>]::MANIFOLD_BYTES,
                manifold_queries: &[<cas_metrics_ $name:lower>]::MANIFOLD_QUERIES,
                hedwig_bytes: &[<cas_metrics_ $name:lower>]::HEDWIG_BYTES,
                hedwig_queries: &[<cas_metrics_ $name:lower>]::HEDWIG_QUERIES,
            };
        }
    };
}

macro_rules! static_cas_local_cache_metrics {
    ($name:ident, $prefix:tt) => {
        paste::paste! {
            mod [<cas_metrics_ $name:lower>] {
                pub static LOCAL_CACHE_HITS_FILES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".local_cache.hits.files"));
                pub static LOCAL_CACHE_HITS_BYTES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".local_cache.hits.bytes"));
                pub static LOCAL_CACHE_MISSES_FILES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".local_cache.misses.files"));
                pub static LOCAL_CACHE_MISSES_BYTES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".local_cache.misses.bytes"));
                pub static LOCAL_LMDB_CACHE_HITS_BLOBS: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".local_lmdb_cache.hits.blobs"));
                pub static LOCAL_LMDB_CACHE_HITS_BYTES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".local_lmdb_cache.hits.bytes"));
                pub static LOCAL_LMDB_CACHE_MISSES_BLOBS: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".local_lmdb_cache.misses.blobs"));
                pub static LOCAL_LMDB_CACHE_MISSES_BYTES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".local_lmdb_cache.misses.bytes"));
                pub static LOCAL_CLOOM_MISSES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".local_cloom.misses.blobs"));
                pub static LOCAL_CLOOM_FALSE_POSITIVES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".local_cloom.false_positives.bytes"));
                pub static LOCAL_CLOOM_TRUE_POSITIVES: ::metrics::Counter = ::metrics::Counter::new_counter(concat!($prefix, ".local_cloom.true_positives.bytes"));
            }

            static $name: $crate::scmstore::metrics::CasLocalCacheMetrics = $crate::scmstore::metrics::CasLocalCacheMetrics {
                local_cache_hits_files: &[<cas_metrics_ $name:lower>]::LOCAL_CACHE_HITS_FILES,
                local_cache_hits_bytes: &[<cas_metrics_ $name:lower>]::LOCAL_CACHE_HITS_BYTES,
                local_cache_misses_files: &[<cas_metrics_ $name:lower>]::LOCAL_CACHE_MISSES_FILES,
                local_cache_misses_bytes: &[<cas_metrics_ $name:lower>]::LOCAL_CACHE_MISSES_BYTES,
                local_lmdb_cache_hits_blobs: &[<cas_metrics_ $name:lower>]::LOCAL_LMDB_CACHE_HITS_BLOBS,
                local_lmdb_cache_hits_bytes: &[<cas_metrics_ $name:lower>]::LOCAL_LMDB_CACHE_HITS_BYTES,
                local_lmdb_cache_misses_blobs: &[<cas_metrics_ $name:lower>]::LOCAL_LMDB_CACHE_MISSES_BLOBS,
                local_lmdb_cache_misses_bytes: &[<cas_metrics_ $name:lower>]::LOCAL_LMDB_CACHE_MISSES_BYTES,
                local_cloom_misses: &[<cas_metrics_ $name:lower>]::LOCAL_CLOOM_MISSES,
                local_cloom_false_positives: &[<cas_metrics_ $name:lower>]::LOCAL_CLOOM_FALSE_POSITIVES,
                local_cloom_true_positives: &[<cas_metrics_ $name:lower>]::LOCAL_CLOOM_TRUE_POSITIVES,
            };
        }
    };
}

pub(crate) use static_cas_backend_metrics;
pub(crate) use static_cas_local_cache_metrics;

impl CasBackendMetrics {
    pub(crate) fn update(&self, stats: &CasFetchedStats) {
        self.zdb_bytes.add(stats.total_bytes_zdb as usize);
        self.zgw_bytes.add(stats.total_bytes_zgw as usize);
        self.manifold_bytes.add(stats.total_bytes_manifold as usize);
        self.hedwig_bytes.add(stats.total_bytes_hedwig as usize);
        self.zdb_queries.add(stats.queries_zdb as usize);
        self.zgw_queries.add(stats.queries_zgw as usize);
        self.manifold_queries.add(stats.queries_manifold as usize);
        self.hedwig_queries.add(stats.queries_hedwig as usize);
    }
}

impl CasLocalCacheMetrics {
    pub(crate) fn update(&self, stats: &CasFetchedStats) {
        self.local_cache_hits_files
            .add(stats.hits_files_local_cache as usize);
        self.local_cache_hits_bytes
            .add(stats.hits_bytes_local_cache as usize);
        self.local_cache_misses_files
            .add(stats.misses_files_local_cache as usize);
        self.local_cache_misses_bytes
            .add(stats.misses_bytes_local_cache as usize);
        self.local_lmdb_cache_hits_blobs
            .add(stats.hits_blobs_local_lmdb_cache as usize);
        self.local_lmdb_cache_hits_bytes
            .add(stats.hits_bytes_local_lmdb_cache as usize);
        self.local_lmdb_cache_misses_blobs
            .add(stats.misses_blobs_local_lmdb_cache as usize);
        self.local_lmdb_cache_misses_bytes
            .add(stats.misses_bytes_local_lmdb_cache as usize);
        self.local_cloom_misses.add(stats.cloom_misses as usize);
        self.local_cloom_false_positives
            .add(stats.cloom_false_positives as usize);
        self.local_cloom_true_positives
            .add(stats.cloom_true_positives as usize);
    }
}
