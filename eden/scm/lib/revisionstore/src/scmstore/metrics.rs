/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ops::AddAssign;

use ::metrics::Counter;

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
