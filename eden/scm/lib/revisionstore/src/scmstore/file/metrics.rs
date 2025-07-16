/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ops::AddAssign;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::scmstore::metrics::ApiMetrics;
use crate::scmstore::metrics::CasBackendMetrics;
use crate::scmstore::metrics::CasLocalCacheMetrics;
use crate::scmstore::metrics::FetchMetrics;
use crate::scmstore::metrics::LocalAndCacheFetchMetrics;
use crate::scmstore::metrics::WriteMetrics;
use crate::scmstore::metrics::namespaced;
use crate::scmstore::metrics::static_cas_backend_metrics;
use crate::scmstore::metrics::static_cas_local_cache_metrics;
use crate::scmstore::metrics::static_fetch_metrics;
use crate::scmstore::metrics::static_local_cache_fetch_metrics;

// DO NOT RENAME: Please be aware that the names of the metrics are used in various parts of the system
static_local_cache_fetch_metrics!(INDEXEDLOG, "scmstore.file.fetch.indexedlog");
static_local_cache_fetch_metrics!(LFS, "scmstore.file.fetch.lfs");
static_local_cache_fetch_metrics!(AUX, "scmstore.file.fetch.aux");
static_fetch_metrics!(EDENAPI, "scmstore.file.fetch.edenapi");
static_fetch_metrics!(CAS, "scmstore.file.fetch.cas");

static_cas_backend_metrics!(CAS_BACKEND, "scmstore.file.fetch.cas");
static_cas_local_cache_metrics!(CAS_LOCAL_CACHE, "scmstore.file.fetch.cas");
static_cas_local_cache_metrics!(CAS_DIRECT_LOCAL_CACHE, "scmstore.file.fetch.cas_direct");

pub(crate) static FILE_STORE_FETCH_METRICS: FileStoreFetchMetrics = FileStoreFetchMetrics {
    indexedlog: &INDEXEDLOG,
    lfs: &LFS,
    aux: &AUX,
    edenapi: &EDENAPI,
    cas: &CAS,
    cas_backend: &CAS_BACKEND,
    cas_local_cache: &CAS_LOCAL_CACHE,
    cas_direct_local_cache: &CAS_DIRECT_LOCAL_CACHE,
};

static_local_cache_fetch_metrics!(INDEXEDLOG_PREFETCH, "scmstore.file.prefetch.indexedlog");
static_local_cache_fetch_metrics!(LFS_PREFETCH, "scmstore.file.prefetch.lfs");
static_local_cache_fetch_metrics!(AUX_PREFETCH, "scmstore.file.prefetch.aux");
static_fetch_metrics!(EDENAPI_PREFETCH, "scmstore.file.prefetch.edenapi");
static_fetch_metrics!(CAS_PREFETCH, "scmstore.file.prefetch.cas");

static_cas_backend_metrics!(CAS_BACKEND_PREFETCH, "scmstore.file.prefetch.cas");
static_cas_local_cache_metrics!(CAS_LOCAL_CACHE_PREFETCH, "scmstore.file.prefetch.cas");
static_cas_local_cache_metrics!(
    CAS_DIRECT_LOCAL_CACHE_PREFETCH,
    "scmstore.file.prefetch.cas_direct"
);

pub(crate) static FILE_STORE_PREFETCH_METRICS: FileStoreFetchMetrics = FileStoreFetchMetrics {
    indexedlog: &INDEXEDLOG_PREFETCH,
    lfs: &LFS_PREFETCH,
    aux: &AUX_PREFETCH,
    edenapi: &EDENAPI_PREFETCH,
    cas: &CAS_PREFETCH,
    cas_backend: &CAS_BACKEND_PREFETCH,
    cas_local_cache: &CAS_LOCAL_CACHE_PREFETCH,
    cas_direct_local_cache: &CAS_DIRECT_LOCAL_CACHE_PREFETCH,
};

pub struct FileStoreFetchMetrics {
    pub(crate) indexedlog: &'static LocalAndCacheFetchMetrics,
    pub(crate) lfs: &'static LocalAndCacheFetchMetrics,
    pub(crate) aux: &'static LocalAndCacheFetchMetrics,
    pub(crate) edenapi: &'static FetchMetrics,
    pub(crate) cas: &'static FetchMetrics,
    pub(crate) cas_backend: &'static CasBackendMetrics,
    pub(crate) cas_local_cache: &'static CasLocalCacheMetrics,
    pub(crate) cas_direct_local_cache: &'static CasLocalCacheMetrics,
}

#[derive(Clone, Debug, Default)]
pub struct FileStoreWriteMetrics {
    /// Writes to the local LFS backend
    pub(crate) lfs: WriteMetrics,

    /// Writes to the local non-LFS backend
    pub(crate) nonlfs: WriteMetrics,

    /// LFS Pointer-only writes (supported only through fallback)
    pub(crate) lfsptr: WriteMetrics,
}

impl AddAssign for FileStoreWriteMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.lfs += rhs.lfs;
        self.nonlfs += rhs.nonlfs;
        self.lfsptr += rhs.lfsptr;
    }
}

impl FileStoreWriteMetrics {
    fn metrics(&self) -> impl Iterator<Item = (String, usize)> + use<> {
        namespaced("lfs", self.lfs.metrics())
            .chain(namespaced("nonlfs", self.nonlfs.metrics()))
            .chain(namespaced("lfsptr", self.lfsptr.metrics()))
    }
}

#[derive(Clone, Debug, Default)]
pub struct FileStoreApiMetrics {
    pub(crate) hg_getfilecontent: ApiMetrics,
    pub(crate) hg_addpending: ApiMetrics,
    pub(crate) hg_commitpending: ApiMetrics,
    pub(crate) hg_get: ApiMetrics,
    pub(crate) hg_getmeta: ApiMetrics,
    pub(crate) hg_refresh: ApiMetrics,
    pub(crate) hg_prefetch: ApiMetrics,
    pub(crate) hg_upload: ApiMetrics,
    pub(crate) hg_getmissing: ApiMetrics,
    pub(crate) hg_add: ApiMetrics,
    pub(crate) hg_flush: ApiMetrics,
    pub(crate) contentdatastore_blob: ApiMetrics,
    pub(crate) contentdatastore_metadata: ApiMetrics,
}

impl AddAssign for FileStoreApiMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.hg_getfilecontent += rhs.hg_getfilecontent;
        self.hg_addpending += rhs.hg_addpending;
        self.hg_commitpending += rhs.hg_commitpending;
        self.hg_get += rhs.hg_get;
        self.hg_getmeta += rhs.hg_getmeta;
        self.hg_refresh += rhs.hg_refresh;
        self.hg_prefetch += rhs.hg_prefetch;
        self.hg_upload += rhs.hg_upload;
        self.hg_getmissing += rhs.hg_getmissing;
        self.hg_add += rhs.hg_add;
        self.hg_flush += rhs.hg_flush;
        self.contentdatastore_blob += rhs.contentdatastore_blob;
        self.contentdatastore_metadata += rhs.contentdatastore_metadata;
    }
}

impl FileStoreApiMetrics {
    fn metrics(&self) -> impl Iterator<Item = (String, usize)> + use<> {
        namespaced("hg_getfilecontent", self.hg_getfilecontent.metrics())
            .chain(namespaced("hg_addpending", self.hg_addpending.metrics()))
            .chain(namespaced(
                "hg_commitpending",
                self.hg_commitpending.metrics(),
            ))
            .chain(namespaced("hg_get", self.hg_get.metrics()))
            .chain(namespaced("hg_getmeta", self.hg_getmeta.metrics()))
            .chain(namespaced("hg_refresh", self.hg_refresh.metrics()))
            .chain(namespaced("hg_prefetch", self.hg_prefetch.metrics()))
            .chain(namespaced("hg_upload", self.hg_upload.metrics()))
            .chain(namespaced("hg_getmissing", self.hg_getmissing.metrics()))
            .chain(namespaced("hg_add", self.hg_add.metrics()))
            .chain(namespaced(
                "contentdatastore_blob",
                self.contentdatastore_blob.metrics(),
            ))
            .chain(namespaced(
                "contentdatastore_metadata",
                self.contentdatastore_metadata.metrics(),
            ))
    }
}

#[derive(Debug, Default, Clone)]
pub struct FileStoreMetrics {
    pub(crate) write: FileStoreWriteMetrics,
    pub(crate) api: FileStoreApiMetrics,
}

impl FileStoreMetrics {
    pub fn new() -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(FileStoreMetrics::default()))
    }

    pub fn metrics(&self) -> impl Iterator<Item = (String, usize)> + use<> {
        namespaced(
            "scmstore.file",
            namespaced("write", self.write.metrics()).chain(namespaced("api", self.api.metrics())),
        )
    }
}
