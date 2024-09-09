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
use crate::scmstore::metrics::ApiMetrics;
use crate::scmstore::metrics::FetchMetrics;
use crate::scmstore::metrics::LocalAndCacheFetchMetrics;
use crate::scmstore::metrics::WriteMetrics;

#[derive(Clone, Debug, Default)]
pub struct FileStoreFetchMetrics {
    pub(crate) indexedlog: LocalAndCacheFetchMetrics,
    pub(crate) lfs: LocalAndCacheFetchMetrics,
    pub(crate) aux: LocalAndCacheFetchMetrics,
    pub(crate) edenapi: FetchMetrics,
    pub(crate) cas: FetchMetrics,
}

impl AddAssign for FileStoreFetchMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.indexedlog += rhs.indexedlog;
        self.lfs += rhs.lfs;
        self.aux += rhs.aux;
        self.edenapi += rhs.edenapi;
        self.cas += rhs.cas;
    }
}

impl FileStoreFetchMetrics {
    fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
        namespaced("indexedlog", self.indexedlog.metrics())
            .chain(namespaced("lfs", self.lfs.metrics()))
            .chain(namespaced("aux", self.aux.metrics()))
            .chain(namespaced("edenapi", self.edenapi.metrics()))
            .chain(namespaced("cas", self.cas.metrics()))
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
    fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
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
    fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
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
    pub(crate) fetch: FileStoreFetchMetrics,
    pub(crate) write: FileStoreWriteMetrics,
    pub(crate) api: FileStoreApiMetrics,
}

impl FileStoreMetrics {
    pub fn new() -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(FileStoreMetrics::default()))
    }

    pub fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
        namespaced(
            "scmstore.file",
            namespaced("fetch", self.fetch.metrics())
                .chain(namespaced("write", self.write.metrics()))
                .chain(namespaced("api", self.api.metrics())),
        )
    }
}

#[cfg(feature = "ods")]
define_stats! {
    prefix = "scmstore.file";
    fetch: dynamic_singleton_counter("fetch.{}", (specific_counter: String)),
}
