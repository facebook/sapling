/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use configmodel::convert::ByteCount;
use configmodel::Config;
use configmodel::ConfigExt;
use edenapi::Builder;
use parking_lot::Mutex;
use progress_model::AggregatingProgressBar;
use regex::Regex;

use crate::contentstore::check_cache_buster;
use crate::fetch_logger::FetchLogger;
use crate::indexedlogauxstore::AuxStore;
use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
use crate::indexedlogdatastore::IndexedLogHgIdDataStoreConfig;
use crate::indexedlogutil::StoreType;
use crate::lfs::LfsRemote;
use crate::lfs::LfsStore;
use crate::scmstore::activitylogger::ActivityLogger;
use crate::scmstore::file::FileStoreMetrics;
use crate::scmstore::FileStore;
use crate::scmstore::TreeStore;
use crate::util::get_indexedlogdatastore_aux_path;
use crate::util::get_indexedlogdatastore_path;
use crate::util::get_local_path;
use crate::ContentStore;
use crate::EdenApiFileStore;
use crate::EdenApiTreeStore;
use crate::ExtStoredPolicy;
use crate::MemcacheStore;

pub struct FileStoreBuilder<'a> {
    config: &'a dyn Config,
    local_path: Option<PathBuf>,
    suffix: Option<PathBuf>,
    correlator: Option<String>,
    store_aux_data: bool,
    override_edenapi: Option<bool>,

    indexedlog_local: Option<Arc<IndexedLogHgIdDataStore>>,
    indexedlog_cache: Option<Arc<IndexedLogHgIdDataStore>>,
    lfs_local: Option<Arc<LfsStore>>,
    lfs_cache: Option<Arc<LfsStore>>,

    edenapi: Option<Arc<EdenApiFileStore>>,
    memcache: Option<Arc<MemcacheStore>>,

    contentstore: Option<Arc<ContentStore>>,
}

impl<'a> FileStoreBuilder<'a> {
    pub fn new(config: &'a dyn Config) -> Self {
        Self {
            config,
            local_path: None,
            suffix: None,
            correlator: None,
            store_aux_data: false,
            override_edenapi: None,
            indexedlog_local: None,
            indexedlog_cache: None,
            lfs_local: None,
            lfs_cache: None,
            edenapi: None,
            memcache: None,
            contentstore: None,
        }
    }

    pub fn local_path(mut self, path: impl AsRef<Path>) -> Self {
        self.local_path = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn suffix(mut self, suffix: impl AsRef<Path>) -> Self {
        self.suffix = Some(suffix.as_ref().to_path_buf());
        self
    }

    pub fn correlator(mut self, correlator: impl ToString) -> Self {
        self.correlator = Some(correlator.to_string());
        self
    }

    pub fn store_aux_data(mut self) -> Self {
        self.store_aux_data = true;
        self
    }

    pub fn override_edenapi(mut self, use_edenapi: bool) -> Self {
        self.override_edenapi = Some(use_edenapi);
        self
    }

    pub fn edenapi(mut self, edenapi: Arc<EdenApiFileStore>) -> Self {
        self.edenapi = Some(edenapi);
        self
    }

    pub fn memcache(mut self, memcache: Arc<MemcacheStore>) -> Self {
        self.memcache = Some(memcache);
        self
    }

    pub fn indexedlog_cache(mut self, indexedlog: Arc<IndexedLogHgIdDataStore>) -> Self {
        self.indexedlog_cache = Some(indexedlog);
        self
    }

    pub fn indexedlog_local(mut self, indexedlog: Arc<IndexedLogHgIdDataStore>) -> Self {
        self.indexedlog_local = Some(indexedlog);
        self
    }

    pub fn lfs_cache(mut self, lfs_cache: Arc<LfsStore>) -> Self {
        self.lfs_cache = Some(lfs_cache);
        self
    }

    pub fn lfs_local(mut self, lfs_local: Arc<LfsStore>) -> Self {
        self.lfs_local = Some(lfs_local);
        self
    }

    pub fn contentstore(mut self, contentstore: Arc<ContentStore>) -> Self {
        self.contentstore = Some(contentstore);
        self
    }

    fn get_extstored_policy(&self) -> Result<ExtStoredPolicy> {
        let enable_lfs = self.config.get_or_default::<bool>("remotefilelog", "lfs")?;
        let extstored_policy = if enable_lfs {
            if self
                .config
                .get_or_default::<bool>("remotefilelog", "useextstored")?
            {
                ExtStoredPolicy::Use
            } else {
                ExtStoredPolicy::Ignore
            }
        } else {
            ExtStoredPolicy::Use
        };
        Ok(extstored_policy)
    }

    fn get_lfs_threshold(&self) -> Result<Option<ByteCount>> {
        let enable_lfs = self.config.get_or_default::<bool>("remotefilelog", "lfs")?;
        let lfs_threshold = if enable_lfs {
            self.config.get_opt::<ByteCount>("lfs", "threshold")?
        } else {
            None
        };

        Ok(lfs_threshold)
    }

    fn get_edenapi_retries(&self) -> i32 {
        self.config
            .get_or_default::<i32>("scmstore", "retries")
            .unwrap_or_default()
    }

    fn use_edenapi(&self) -> Result<bool> {
        Ok(if let Some(use_edenapi) = self.override_edenapi {
            use_edenapi
        } else {
            self.edenapi.is_some() || use_edenapi_via_config(self.config)?
        })
    }

    fn use_lfs(&self) -> Result<bool> {
        Ok(self.get_lfs_threshold()?.is_some())
    }

    fn build_edenapi(&self) -> Result<Arc<EdenApiFileStore>> {
        let client = Builder::from_config(self.config)?.build()?;

        Ok(EdenApiFileStore::new(client))
    }

    pub fn build_indexedlog_local(&self) -> Result<Option<Arc<IndexedLogHgIdDataStore>>> {
        Ok(if let Some(local_path) = self.local_path.clone() {
            let local_path = get_local_path(local_path, &self.suffix)?;
            let config = IndexedLogHgIdDataStoreConfig {
                max_log_count: None,
                max_bytes_per_log: None,
                max_bytes: None,
            };
            Some(Arc::new(IndexedLogHgIdDataStore::new(
                get_indexedlogdatastore_path(&local_path)?,
                self.get_extstored_policy()?,
                &config,
                StoreType::Local,
            )?))
        } else {
            None
        })
    }

    pub fn build_indexedlog_cache(&self) -> Result<Option<Arc<IndexedLogHgIdDataStore>>> {
        let cache_path = match cache_path(self.config, &self.suffix)? {
            Some(p) => p,
            None => return Ok(None),
        };

        let max_log_count = self
            .config
            .get_opt::<u8>("indexedlog", "data.max-log-count")?;
        let max_bytes_per_log = self
            .config
            .get_opt::<ByteCount>("indexedlog", "data.max-bytes-per-log")?;
        let max_bytes = self
            .config
            .get_opt::<ByteCount>("remotefilelog", "cachelimit")?;
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count,
            max_bytes_per_log,
            max_bytes,
        };
        Ok(Some(Arc::new(IndexedLogHgIdDataStore::new(
            get_indexedlogdatastore_path(&cache_path)?,
            self.get_extstored_policy()?,
            &config,
            StoreType::Shared,
        )?)))
    }

    pub fn build_aux_local(&self) -> Result<Option<Arc<AuxStore>>> {
        Ok(if let Some(local_path) = self.local_path.clone() {
            let local_path = get_local_path(local_path, &self.suffix)?;
            let local_path = get_indexedlogdatastore_aux_path(&local_path)?;
            Some(Arc::new(AuxStore::new(
                local_path,
                self.config,
                StoreType::Local,
            )?))
        } else {
            None
        })
    }

    pub fn build_aux_cache(&self) -> Result<Option<Arc<AuxStore>>> {
        let cache_path = match cache_path(self.config, &self.suffix)? {
            Some(p) => p,
            None => return Ok(None),
        };

        let cache_path = get_indexedlogdatastore_aux_path(&cache_path)?;
        Ok(Some(Arc::new(AuxStore::new(
            cache_path,
            self.config,
            StoreType::Shared,
        )?)))
    }

    pub fn build_lfs_local(&self) -> Result<Option<Arc<LfsStore>>> {
        if !self.use_lfs()? {
            return Ok(None);
        }

        Ok(if let Some(local_path) = self.local_path.clone() {
            let local_path = get_local_path(local_path, &self.suffix)?;
            Some(Arc::new(LfsStore::local(&local_path, self.config)?))
        } else {
            None
        })
    }

    pub fn build_lfs_cache(&self) -> Result<Option<Arc<LfsStore>>> {
        if !self.use_lfs()? {
            return Ok(None);
        }

        let cache_path = match cache_path(self.config, &self.suffix)? {
            Some(p) => p,
            None => return Ok(None),
        };

        Ok(Some(Arc::new(LfsStore::shared(&cache_path, self.config)?)))
    }

    pub fn build(mut self) -> Result<FileStore> {
        tracing::trace!(target: "revisionstore::filestore", "checking cache");
        if self.contentstore.is_none() {
            if let Some(cache_path) = cache_path(self.config, &self.suffix)? {
                check_cache_buster(&self.config, &cache_path);
            }
        }

        tracing::trace!(target: "revisionstore::filestore", "processing extstored policy");
        let extstored_policy = self.get_extstored_policy()?;

        tracing::trace!(target: "revisionstore::filestore", "processing lfs threshold");
        let lfs_threshold_bytes = self.get_lfs_threshold()?.map(|b| b.value());

        let edenapi_retries = self.get_edenapi_retries();

        tracing::trace!(target: "revisionstore::filestore", "processing local");
        let indexedlog_local = if let Some(indexedlog_local) = self.indexedlog_local.take() {
            Some(indexedlog_local)
        } else {
            self.build_indexedlog_local()?
        };

        tracing::trace!(target: "revisionstore::filestore", "processing cache");
        let indexedlog_cache = if let Some(indexedlog_cache) = self.indexedlog_cache.take() {
            Some(indexedlog_cache)
        } else {
            self.build_indexedlog_cache()?
        };

        tracing::trace!(target: "revisionstore::filestore", "processing lfs local");
        let lfs_local = if let Some(lfs_local) = self.lfs_local.take() {
            Some(lfs_local)
        } else {
            self.build_lfs_local()?
        };

        tracing::trace!(target: "revisionstore::filestore", "processing lfs cache");
        let lfs_cache = if let Some(lfs_cache) = self.lfs_cache.take() {
            Some(lfs_cache)
        } else {
            self.build_lfs_cache()?
        };

        tracing::trace!(target: "revisionstore::filestore", "processing aux data");
        let (aux_local, aux_cache) = if self.store_aux_data {
            let aux_local = self.build_aux_local()?;
            let aux_cache = self.build_aux_cache()?;
            (aux_local, aux_cache)
        } else {
            (None, None)
        };

        tracing::trace!(target: "revisionstore::filestore", "processing lfs remote");
        let lfs_remote = if self.use_lfs()? {
            if let Some(ref lfs_cache) = lfs_cache {
                // TODO(meyer): Refactor upload functionality so we don't need to use LfsRemote with it's own references to the
                // underlying stores.
                Some(Arc::new(LfsRemote::new(
                    lfs_cache.clone(),
                    lfs_local.clone(),
                    self.config,
                    self.correlator.take(),
                )?))
            } else {
                None
            }
        } else {
            None
        };

        let memcache = self.memcache.take();

        tracing::trace!(target: "revisionstore::filestore", "processing edenapi");
        let edenapi = if self.use_edenapi()? {
            if let Some(edenapi) = self.edenapi.take() {
                Some(edenapi)
            } else {
                Some(self.build_edenapi()?)
            }
        } else {
            None
        };

        tracing::trace!(target: "revisionstore::filestore", "processing contentstore");
        let contentstore = if self
            .config
            .get_or_default::<bool>("scmstore", "contentstorefallback")?
        {
            self.contentstore
        } else {
            None
        };

        tracing::trace!(target: "revisionstore::filestore", "processing fetch logger");
        let logging_regex = self
            .config
            .get_opt::<String>("remotefilelog", "undesiredfileregex")?
            .map(|s| Regex::new(&s))
            .transpose()?;
        let fetch_logger = Some(Arc::new(FetchLogger::new(logging_regex)));

        let allow_write_lfs_ptrs = self
            .config
            .get_or_default::<bool>("scmstore", "lfsptrwrites")?;

        let prefer_computing_aux_data = self
            .config
            .get_or_default::<bool>("scmstore", "prefercomputingauxdata")?;

        let activity_logger =
            if let Some(path) = self.config.get_opt::<String>("scmstore", "activitylog")? {
                let f = std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(path)?;
                Some(Arc::new(Mutex::new(ActivityLogger::new(f))))
            } else {
                None
            };

        tracing::trace!(target: "revisionstore::filestore", "constructing FileStore");
        Ok(FileStore {
            extstored_policy,
            lfs_threshold_bytes,
            edenapi_retries,
            allow_write_lfs_ptrs,
            prefer_computing_aux_data,

            indexedlog_local,
            lfs_local,

            indexedlog_cache,
            lfs_cache,
            cache_to_local_cache: true,

            memcache,
            cache_to_memcache: true,

            edenapi,
            lfs_remote,

            activity_logger,
            contentstore,
            fetch_logger,
            metrics: FileStoreMetrics::new(),

            aux_local,
            aux_cache,

            creation_time: Instant::now(),
            lfs_progress: AggregatingProgressBar::new("fetching", "LFS"),
            flush_on_drop: true,
        })
    }
}

// Return remotefilelog cache path, or None if there is no cache path
// (e.g. because we have no repo name).
fn cache_path(
    config: &dyn Config,
    suffix: &Option<PathBuf>,
) -> Result<Option<PathBuf>, crate::Error> {
    match crate::util::get_cache_path(config, suffix) {
        Ok(p) => Ok(Some(p)),
        Err(crate::Error::ConfigNotSet(_)) => Ok(None),
        Err(err) => Err(err),
    }
}

pub struct TreeStoreBuilder<'a> {
    config: &'a dyn Config,
    local_path: Option<PathBuf>,
    suffix: Option<PathBuf>,
    override_edenapi: Option<bool>,

    indexedlog_local: Option<Arc<IndexedLogHgIdDataStore>>,
    indexedlog_cache: Option<Arc<IndexedLogHgIdDataStore>>,
    edenapi: Option<Arc<EdenApiTreeStore>>,
    memcache: Option<Arc<MemcacheStore>>,
    contentstore: Option<Arc<ContentStore>>,
    filestore: Option<Arc<FileStore>>,
}

impl<'a> TreeStoreBuilder<'a> {
    pub fn new(config: &'a dyn Config) -> Self {
        Self {
            config,
            local_path: None,
            suffix: None,
            override_edenapi: None,
            indexedlog_local: None,
            indexedlog_cache: None,
            edenapi: None,
            memcache: None,
            contentstore: None,
            filestore: None,
        }
    }

    pub fn local_path(mut self, path: impl AsRef<Path>) -> Self {
        self.local_path = Some(path.as_ref().to_path_buf());
        self
    }

    // TODO(meyer): Can we remove this since we have seprate builders for files and trees?
    // Is this configurable somewhere we can directly check from Config instead of having the
    // caller pass in, or is it just hardcoded elsewhere and we should hardcode it here?
    /// Cache path suffix for the associated indexedlog. For files, this will not be given.
    /// For trees, it will be "manifests".
    pub fn suffix(mut self, suffix: impl AsRef<Path>) -> Self {
        self.suffix = Some(suffix.as_ref().to_path_buf());
        self
    }

    pub fn edenapi(mut self, edenapi: Arc<EdenApiTreeStore>) -> Self {
        self.edenapi = Some(edenapi);
        self
    }

    pub fn memcache(mut self, memcache: Arc<MemcacheStore>) -> Self {
        self.memcache = Some(memcache);
        self
    }

    pub fn indexedlog_cache(mut self, indexedlog: Arc<IndexedLogHgIdDataStore>) -> Self {
        self.indexedlog_cache = Some(indexedlog);
        self
    }

    pub fn indexedlog_local(mut self, indexedlog: Arc<IndexedLogHgIdDataStore>) -> Self {
        self.indexedlog_local = Some(indexedlog);
        self
    }

    pub fn contentstore(mut self, contentstore: Arc<ContentStore>) -> Self {
        self.contentstore = Some(contentstore);
        self
    }

    pub fn override_edenapi(mut self, use_edenapi: bool) -> Self {
        self.override_edenapi = Some(use_edenapi);
        self
    }

    pub fn filestore(mut self, filestore: Arc<FileStore>) -> Self {
        self.filestore = Some(filestore);
        self
    }

    fn use_edenapi(&self) -> Result<bool> {
        Ok(if let Some(use_edenapi) = self.override_edenapi {
            use_edenapi
        } else {
            self.edenapi.is_some() || use_edenapi_via_config(self.config)?
        })
    }

    fn build_edenapi(&self) -> Result<Arc<EdenApiTreeStore>> {
        let client = Builder::from_config(self.config)?.build()?;

        Ok(EdenApiTreeStore::new(client))
    }

    pub fn build_indexedlog_local(&self) -> Result<Option<Arc<IndexedLogHgIdDataStore>>> {
        Ok(if let Some(local_path) = self.local_path.clone() {
            let local_path = get_local_path(local_path, &self.suffix)?;
            let config = IndexedLogHgIdDataStoreConfig {
                max_log_count: None,
                max_bytes_per_log: None,
                max_bytes: None,
            };
            Some(Arc::new(IndexedLogHgIdDataStore::new(
                get_indexedlogdatastore_path(&local_path)?,
                ExtStoredPolicy::Use,
                &config,
                StoreType::Local,
            )?))
        } else {
            None
        })
    }

    pub fn build_indexedlog_cache(&self) -> Result<Option<Arc<IndexedLogHgIdDataStore>>> {
        let cache_path = match cache_path(self.config, &self.suffix)? {
            Some(p) => p,
            None => return Ok(None),
        };

        let max_log_count = self
            .config
            .get_opt::<u8>("indexedlog", "manifest.max-log-count")?;
        let max_bytes_per_log = self
            .config
            .get_opt::<ByteCount>("indexedlog", "manifest.max-bytes-per-log")?;
        let max_bytes = self
            .config
            .get_opt::<ByteCount>("remotefilelog", "manifestlimit")?;
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count,
            max_bytes_per_log,
            max_bytes,
        };

        Ok(Some(Arc::new(IndexedLogHgIdDataStore::new(
            get_indexedlogdatastore_path(&cache_path)?,
            ExtStoredPolicy::Use,
            &config,
            StoreType::Shared,
        )?)))
    }

    pub fn build(mut self) -> Result<TreeStore> {
        // TODO(meyer): Clean this up, just copied and pasted from the other version & did some ugly hacks to get this
        // (the EdenApiAdapter stuff needs to be fixed in particular)
        tracing::trace!(target: "revisionstore::treestore", "checking cache");
        if self.contentstore.is_none() {
            if let Some(cache_path) = cache_path(self.config, &self.suffix)? {
                check_cache_buster(&self.config, &cache_path);
            }
        }

        tracing::trace!(target: "revisionstore::treestore", "processing local");
        let indexedlog_local = if let Some(indexedlog_local) = self.indexedlog_local.take() {
            Some(indexedlog_local)
        } else {
            self.build_indexedlog_local()?
        };

        tracing::trace!(target: "revisionstore::treestore", "processing cache");
        let indexedlog_cache = if let Some(indexedlog_cache) = self.indexedlog_cache.take() {
            Some(indexedlog_cache)
        } else {
            self.build_indexedlog_cache()?
        };

        let memcache = self.memcache.take();

        tracing::trace!(target: "revisionstore::treestore", "processing edenapi");
        let edenapi = if self.use_edenapi()? {
            if let Some(edenapi) = self.edenapi.take() {
                Some(edenapi)
            } else {
                Some(self.build_edenapi()?)
            }
        } else {
            None
        };

        tracing::trace!(target: "revisionstore::treestore", "processing contentstore");
        let contentstore = if self
            .config
            .get_or_default::<bool>("scmstore", "contentstorefallback")?
        {
            self.contentstore
        } else {
            None
        };

        tracing::trace!(target: "revisionstore::treestore", "constructing TreeStore");
        Ok(TreeStore {
            indexedlog_local,

            indexedlog_cache,
            cache_to_local_cache: true,

            memcache,
            cache_to_memcache: true,

            edenapi,

            contentstore,
            filestore: self.filestore,

            creation_time: Instant::now(),
            flush_on_drop: true,
        })
    }
}

fn use_edenapi_via_config(config: &dyn Config) -> Result<bool> {
    let mut use_edenapi: bool = config.get_or_default("remotefilelog", "http")?;
    if use_edenapi {
        // If paths.default is not set, there is no server repo. Therefore, do
        // not use edenapi.
        let path: Option<String> = config.get_opt("paths", "default")?;
        if path.is_none() {
            use_edenapi = false;
        }
    }
    Ok(use_edenapi)
}
