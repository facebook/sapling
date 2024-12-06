/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::scmstore::metrics::static_cas_backend_metrics;
use crate::scmstore::metrics::static_fetch_metrics;
use crate::scmstore::metrics::static_local_cache_fetch_metrics;
use crate::scmstore::metrics::CasBackendMetrics;
use crate::scmstore::metrics::FetchMetrics;
use crate::scmstore::metrics::LocalAndCacheFetchMetrics;

static_local_cache_fetch_metrics!(INDEXEDLOG, "scmstore.tree.fetch.indexedlog");
static_fetch_metrics!(EDENAPI, "scmstore.tree.fetch.edenapi");
static_fetch_metrics!(CAS, "scmstore.tree.fetch.cas");

static_cas_backend_metrics!(CAS_BACKEND, "scmstore.tree.fetch.cas");

pub(crate) static TREE_STORE_FETCH_METRICS: TreeStoreFetchMetrics = TreeStoreFetchMetrics {
    indexedlog: &INDEXEDLOG,
    edenapi: &EDENAPI,
    cas: &CAS,
    cas_backend: &CAS_BACKEND,
};

pub struct TreeStoreFetchMetrics {
    pub(crate) indexedlog: &'static LocalAndCacheFetchMetrics,
    pub(crate) edenapi: &'static FetchMetrics,
    pub(crate) cas: &'static FetchMetrics,
    pub(crate) cas_backend: &'static CasBackendMetrics,
}
