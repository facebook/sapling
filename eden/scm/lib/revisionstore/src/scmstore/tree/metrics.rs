/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use types::CasFetchedStats;

use crate::scmstore::metrics::static_cas_backend_metrics;
use crate::scmstore::metrics::static_fetch_metrics;
use crate::scmstore::metrics::static_local_cache_fetch_metrics;
use crate::scmstore::metrics::CasBackendMetrics;
use crate::scmstore::metrics::FetchMetrics;
use crate::scmstore::metrics::LocalAndCacheFetchMetrics;

static_local_cache_fetch_metrics!(INDEXEDLOG, "scmstore.tree.fetch.indexedlog");
static_local_cache_fetch_metrics!(AUX, "scmstore.tree.fetch.aux");
static_fetch_metrics!(EDENAPI, "scmstore.tree.fetch.edenapi");
static_fetch_metrics!(CAS, "scmstore.tree.fetch.cas");

static_cas_backend_metrics!(CAS_BACKEND, "scmstore.tree.fetch.cas");

pub(crate) static TREE_STORE_FETCH_METRICS: TreeStoreFetchMetrics = TreeStoreFetchMetrics {
    indexedlog: &INDEXEDLOG,
    edenapi: &EDENAPI,
    aux: &AUX,
    cas: &CAS,
    cas_backend: &CAS_BACKEND,
};

pub struct TreeStoreFetchMetrics {
    pub(crate) indexedlog: &'static LocalAndCacheFetchMetrics,
    pub(crate) edenapi: &'static FetchMetrics,
    pub(crate) aux: &'static LocalAndCacheFetchMetrics,
    pub(crate) cas: &'static FetchMetrics,
    pub(crate) cas_backend: &'static CasBackendMetrics,
}

impl TreeStoreFetchMetrics {
    pub(crate) fn update_cas_backend_stats(&self, stats: &CasFetchedStats) {
        self.cas_backend.zdb_bytes(stats.total_bytes_zdb);
        self.cas_backend.zgw_bytes(stats.total_bytes_zgw);
        self.cas_backend.manifold_bytes(stats.total_bytes_manifold);
        self.cas_backend.hedwig_bytes(stats.total_bytes_hedwig);
        self.cas_backend.zdb_queries(stats.queries_zdb);
        self.cas_backend.zgw_queries(stats.queries_zgw);
        self.cas_backend.manifold_queries(stats.queries_manifold);
        self.cas_backend.hedwig_queries(stats.queries_hedwig);

        self.cas_backend
            .local_cache_hits_files(stats.hits_files_local_cache);

        self.cas_backend
            .local_cache_hits_bytes(stats.hits_bytes_local_cache);

        self.cas_backend
            .local_cache_misses_files(stats.misses_files_local_cache);

        self.cas_backend
            .local_cache_misses_bytes(stats.misses_bytes_local_cache);

        self.cas_backend
            .local_lmdb_cache_hits_blobs(stats.hits_blobs_local_lmdb_cache);

        self.cas_backend
            .local_lmdb_cache_hits_bytes(stats.hits_bytes_local_lmdb_cache);

        self.cas_backend
            .local_lmdb_cache_misses_blobs(stats.misses_blobs_local_lmdb_cache);

        self.cas_backend
            .local_lmdb_cache_misses_bytes(stats.misses_bytes_local_lmdb_cache);

        self.cas_backend.local_cloom_misses(stats.cloom_misses);

        self.cas_backend
            .local_cloom_false_positives(stats.cloom_false_positives);

        self.cas_backend
            .local_cloom_true_positives(stats.cloom_true_positives);
    }
}
