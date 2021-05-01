/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;

use configparser::config::ConfigSet;
use edenapi::Builder;

use crate::{
    contentstore::check_cache_buster,
    indexedlogdatastore::{IndexedLogDataStoreType, IndexedLogHgIdDataStore},
    scmstore::TreeStore,
    util::{get_cache_path, get_indexedlogdatastore_path, get_local_path, get_repo_name},
    ContentStore, EdenApiTreeStore, ExtStoredPolicy, MemcacheStore,
};

pub struct TreeStoreBuilder<'a> {
    config: &'a ConfigSet,
    local_path: Option<PathBuf>,
    suffix: Option<&'a Path>,

    indexedlog_local: Option<Arc<IndexedLogHgIdDataStore>>,
    indexedlog_cache: Option<Arc<IndexedLogHgIdDataStore>>,
    edenapi: Option<Arc<EdenApiTreeStore>>,
    memcache: Option<Arc<MemcacheStore>>,

    contentstore: Option<Arc<ContentStore>>,
}

impl<'a> TreeStoreBuilder<'a> {
    pub fn new(config: &'a ConfigSet) -> Self {
        Self {
            config,
            local_path: None,
            suffix: None,
            indexedlog_local: None,
            indexedlog_cache: None,
            edenapi: None,
            memcache: None,
            contentstore: None,
        }
    }

    pub fn local_path(mut self, path: impl AsRef<Path>) -> Self {
        self.local_path = Some(path.as_ref().to_path_buf());
        self
    }

    // TODO(meyer): Can we remove this since we have seprate builders for files and trees?
    // Is this configurable somewhere we can directly check from ConfigSet instead of having the
    // caller pass in, or is it just hardcoded elsewhere and we should hardcode it here?
    /// Cache path suffix for the associated indexedlog. For files, this will not be given.
    /// For trees, it will be "manifests".
    pub fn suffix(mut self, suffix: &'a Path) -> Self {
        self.suffix = Some(suffix);
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

    fn use_edenapi(&self) -> Result<bool> {
        Ok(self.config.get_or_default::<bool>("treemanifest", "http")?)
    }

    fn build_edenapi(&self) -> Result<Arc<EdenApiTreeStore>> {
        let reponame = get_repo_name(self.config)?;
        let client = Builder::from_config(self.config)?.build()?;

        Ok(EdenApiTreeStore::new(reponame, client, None))
    }

    fn build_indexedlog_local(&self, path: PathBuf) -> Result<Arc<IndexedLogHgIdDataStore>> {
        let local_path = get_local_path(path, &self.suffix)?;
        Ok(Arc::new(IndexedLogHgIdDataStore::new(
            get_indexedlogdatastore_path(&local_path)?,
            ExtStoredPolicy::Use,
            self.config,
            IndexedLogDataStoreType::Local,
        )?))
    }

    fn build_indexedlog_cache(&self) -> Result<Arc<IndexedLogHgIdDataStore>> {
        let cache_path = get_cache_path(self.config, &self.suffix)?;
        Ok(Arc::new(IndexedLogHgIdDataStore::new(
            get_indexedlogdatastore_path(&cache_path)?,
            ExtStoredPolicy::Use,
            self.config,
            IndexedLogDataStoreType::Shared,
        )?))
    }

    pub fn build(mut self) -> Result<TreeStore> {
        // TODO(meyer): Clean this up, just copied and pasted from the other version & did some ugly hacks to get this
        // (the EdenApiAdapter stuff needs to be fixed in particular)
        if self.contentstore.is_none() {
            let cache_path = get_cache_path(self.config, &self.suffix)?;
            check_cache_buster(&self.config, &cache_path);
        }

        let indexedlog_local = if let Some(local_path) = self.local_path.take() {
            if let Some(indexedlog_local) = self.indexedlog_local.take() {
                Some(indexedlog_local)
            } else {
                Some(self.build_indexedlog_local(local_path)?)
            }
        } else {
            None
        };

        let indexedlog_cache = if let Some(indexedlog_cache) = self.indexedlog_cache.take() {
            Some(indexedlog_cache)
        } else {
            Some(self.build_indexedlog_cache()?)
        };

        let memcache = self.memcache.take();

        let edenapi = if self.use_edenapi()? {
            if let Some(edenapi) = self.edenapi.take() {
                Some(edenapi)
            } else {
                Some(self.build_edenapi()?)
            }
        } else {
            None
        };

        let contentstore = self.contentstore;

        Ok(TreeStore {
            indexedlog_local,

            indexedlog_cache,
            cache_to_local_cache: true,

            memcache,
            cache_to_memcache: true,

            edenapi,

            contentstore,
        })
    }
}
