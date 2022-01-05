/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::AddAssign;

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

    pub(crate) fn metrics(&self) -> impl Iterator<Item = (&'static str, usize)> {
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

impl LocalAndCacheFetchMetrics {
    pub(crate) fn store(&mut self, typ: StoreType) -> &mut FetchMetrics {
        match typ {
            StoreType::Local => &mut self.local,
            StoreType::Shared => &mut self.cache,
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
        std::array::IntoIter::new([("items", self.items), ("ok", self.ok), ("err", self.err)])
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
        std::array::IntoIter::new([
            ("calls", self.calls),
            ("keys", self.keys),
            ("singles", self.singles),
        ])
        .filter(|&(_, v)| v != 0)
    }
}
