/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::AddAssign;
use std::sync::Arc;

use parking_lot::RwLock;
#[cfg(feature = "ods")]
use stats::prelude::*;

use crate::scmstore::metrics::namespaced;
use crate::scmstore::metrics::FetchMetrics;
use crate::scmstore::metrics::LocalAndCacheFetchMetrics;

#[derive(Clone, Debug, Default)]
pub struct TreeStoreFetchMetrics {
    pub(crate) indexedlog: LocalAndCacheFetchMetrics,
    pub(crate) aux: LocalAndCacheFetchMetrics,
    pub(crate) edenapi: FetchMetrics,
}

impl AddAssign for TreeStoreFetchMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.indexedlog += rhs.indexedlog;
        self.aux += rhs.aux;
        self.edenapi += rhs.edenapi;
    }
}

impl TreeStoreFetchMetrics {
    fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
        namespaced("indexedlog", self.indexedlog.metrics())
            .chain(namespaced("aux", self.aux.metrics()))
            .chain(namespaced("edenapi", self.edenapi.metrics()))
    }

    /// Update ODS stats.
    /// This assumes that fbinit was called higher up the stack.
    /// It is meant to be used when called from eden which uses the `revisionstore` with
    /// the `ods` feature flag.
    #[cfg(feature = "ods")]
    pub(crate) fn update_ods(&self) -> anyhow::Result<()> {
        for (metric, value) in self.metrics() {
            // SAFETY: this is called from C++ and was init'd there
            unsafe {
                let fb = fbinit::assume_init();
                STATS::fetch.increment_value(fb, value.try_into()?, (metric,));
            }
        }
        Ok(())
    }

    #[cfg(not(feature = "ods"))]
    pub(crate) fn update_ods(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
pub struct TreeStoreMetrics {
    pub(crate) fetch: TreeStoreFetchMetrics,
}

impl TreeStoreMetrics {
    pub fn new() -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(TreeStoreMetrics::default()))
    }

    pub fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
        namespaced("scmstore.tree", namespaced("fetch", self.fetch.metrics()))
    }
}

#[cfg(feature = "ods")]
define_stats! {
    prefix = "scmstore.tree";
    fetch: dynamic_singleton_counter("fetch.{}", (specific_counter: String)),
}
