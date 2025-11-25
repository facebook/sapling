/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::scmstore::metrics::FetchMetrics;
use crate::scmstore::metrics::LocalAndCacheFetchMetrics;
use crate::scmstore::metrics::static_fetch_metrics;
use crate::scmstore::metrics::static_local_cache_fetch_metrics;

static_local_cache_fetch_metrics!(INDEXEDLOG, "scmstore.tree.fetch.indexedlog");
static_local_cache_fetch_metrics!(AUX, "scmstore.tree.fetch.aux");
static_fetch_metrics!(EDENAPI, "scmstore.tree.fetch.edenapi");

pub(crate) static TREE_STORE_FETCH_METRICS: TreeStoreFetchMetrics = TreeStoreFetchMetrics {
    indexedlog: &INDEXEDLOG,
    edenapi: &EDENAPI,
    aux: &AUX,
};

static_local_cache_fetch_metrics!(INDEXEDLOG_PREFETCH, "scmstore.tree.prefetch.indexedlog");
static_local_cache_fetch_metrics!(AUX_PREFETCH, "scmstore.tree.prefetch.aux");
static_fetch_metrics!(EDENAPI_PREFETCH, "scmstore.tree.prefetch.edenapi");

pub(crate) static TREE_STORE_PREFETCH_METRICS: TreeStoreFetchMetrics = TreeStoreFetchMetrics {
    indexedlog: &INDEXEDLOG_PREFETCH,
    edenapi: &EDENAPI_PREFETCH,
    aux: &AUX_PREFETCH,
};

pub struct TreeStoreFetchMetrics {
    pub(crate) indexedlog: &'static LocalAndCacheFetchMetrics,
    pub(crate) edenapi: &'static FetchMetrics,
    pub(crate) aux: &'static LocalAndCacheFetchMetrics,
}
