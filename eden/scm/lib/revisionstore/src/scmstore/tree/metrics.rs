/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::scmstore::metrics::CasBackendMetrics;
use crate::scmstore::metrics::CasLocalCacheMetrics;
use crate::scmstore::metrics::FetchMetrics;
use crate::scmstore::metrics::LocalAndCacheFetchMetrics;
use crate::scmstore::metrics::static_cas_backend_metrics;
use crate::scmstore::metrics::static_cas_local_cache_metrics;
use crate::scmstore::metrics::static_fetch_metrics;
use crate::scmstore::metrics::static_local_cache_fetch_metrics;

static_local_cache_fetch_metrics!(INDEXEDLOG, "scmstore.tree.fetch.indexedlog");
static_local_cache_fetch_metrics!(AUX, "scmstore.tree.fetch.aux");
static_fetch_metrics!(EDENAPI, "scmstore.tree.fetch.edenapi");
static_fetch_metrics!(CAS, "scmstore.tree.fetch.cas");

static_cas_backend_metrics!(CAS_BACKEND, "scmstore.tree.fetch.cas");
static_cas_local_cache_metrics!(CAS_LOCAL_CACHE, "scmstore.tree.fetch.cas");
static_cas_local_cache_metrics!(CAS_DIRECT_LOCAL_CACHE, "scmstore.tree.fetch.cas_direct");

pub(crate) static TREE_STORE_FETCH_METRICS: TreeStoreFetchMetrics = TreeStoreFetchMetrics {
    indexedlog: &INDEXEDLOG,
    edenapi: &EDENAPI,
    aux: &AUX,
    cas: &CAS,
    cas_backend: &CAS_BACKEND,
    cas_local_cache: &CAS_LOCAL_CACHE,
    cas_direct_local_cache: &CAS_DIRECT_LOCAL_CACHE,
};

static_local_cache_fetch_metrics!(INDEXEDLOG_PREFETCH, "scmstore.tree.prefetch.indexedlog");
static_local_cache_fetch_metrics!(AUX_PREFETCH, "scmstore.tree.prefetch.aux");
static_fetch_metrics!(EDENAPI_PREFETCH, "scmstore.tree.prefetch.edenapi");
static_fetch_metrics!(CAS_PREFETCH, "scmstore.tree.prefetch.cas");

static_cas_backend_metrics!(CAS_BACKEND_PREFETCH, "scmstore.tree.prefetch.cas");
static_cas_local_cache_metrics!(CAS_LOCAL_CACHE_PREFETCH, "scmstore.tree.prefetch.cas");
static_cas_local_cache_metrics!(
    CAS_DIRECT_LOCAL_CACHE_PREFETCH,
    "scmstore.tree.prefetch.cas_direct"
);

pub(crate) static TREE_STORE_PREFETCH_METRICS: TreeStoreFetchMetrics = TreeStoreFetchMetrics {
    indexedlog: &INDEXEDLOG_PREFETCH,
    edenapi: &EDENAPI_PREFETCH,
    aux: &AUX_PREFETCH,
    cas: &CAS_PREFETCH,
    cas_backend: &CAS_BACKEND_PREFETCH,
    cas_local_cache: &CAS_LOCAL_CACHE_PREFETCH,
    cas_direct_local_cache: &CAS_DIRECT_LOCAL_CACHE_PREFETCH,
};

pub struct TreeStoreFetchMetrics {
    pub(crate) indexedlog: &'static LocalAndCacheFetchMetrics,
    pub(crate) edenapi: &'static FetchMetrics,
    pub(crate) aux: &'static LocalAndCacheFetchMetrics,
    pub(crate) cas: &'static FetchMetrics,
    pub(crate) cas_backend: &'static CasBackendMetrics,
    pub(crate) cas_local_cache: &'static CasLocalCacheMetrics,
    pub(crate) cas_direct_local_cache: &'static CasLocalCacheMetrics,
}
