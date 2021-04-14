/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Result};

use configparser::{config::ConfigSet, convert::ByteCount};
use edenapi::{Builder, Client};
use edenapi_types::FileEntry;
use types::Key;

use crate::{
    contentstore::check_cache_buster,
    indexedlogdatastore::{Entry, IndexedLogDataStoreType, IndexedLogHgIdDataStore},
    scmstore::{
        lfs::lfs_threshold_filtermap_fn, BoxedReadStore, BoxedWriteStore, EdenApiAdapter, Fallback,
        FallbackCache, FilterMapStore, LegacyDatastore, StoreFile,
    },
    util::{get_cache_path, get_indexedlogdatastore_path, get_repo_name},
    ContentStore, EdenApiFileStore, ExtStoredPolicy,
};

pub struct FileScmStoreBuilder<'a> {
    config: &'a ConfigSet,
    suffix: Option<PathBuf>,
    shared_indexedlog: Option<Arc<IndexedLogHgIdDataStore>>,
    shared_edenapi: Option<Arc<EdenApiFileStore>>,
    legacy_datastore: Option<LegacyDatastore<Arc<ContentStore>>>,
}

impl<'a> FileScmStoreBuilder<'a> {
    pub fn new(config: &'a ConfigSet) -> Self {
        Self {
            config,
            suffix: None,
            shared_indexedlog: None,
            shared_edenapi: None,
            legacy_datastore: None,
        }
    }

    // TODO(meyer): Can we remove this since we have seprate builders for files and trees?
    // Is this configurable somewhere we can directly check from ConfigSet instead of having the
    // caller pass in, or is it just hardcoded elsewhere and we should hardcode it here?
    /// Cache path suffix for the associated indexedlog. For files, this will not be given.
    /// For trees, it will be "manifests".
    pub fn suffix(mut self, suffix: impl AsRef<Path>) -> Self {
        self.suffix = Some(suffix.as_ref().to_path_buf());
        self
    }

    pub fn shared_edenapi(mut self, shared_edenapi: Arc<EdenApiFileStore>) -> Self {
        self.shared_edenapi = Some(shared_edenapi);
        self
    }

    pub fn legacy_fallback(mut self, legacy: LegacyDatastore<Arc<ContentStore>>) -> Self {
        self.legacy_datastore = Some(legacy);
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

    fn filtered_indexedlog(
        &self,
        indexedlog: Arc<IndexedLogHgIdDataStore>,
    ) -> Result<BoxedWriteStore<Key, Entry>> {
        let maybe_filtered = if let Some(lfs_threshold) = self.get_lfs_threshold()? {
            Arc::new(FilterMapStore {
                // See [`revisionstore::lfs::LfsMultiplexer`]'s `HgIdMutableDeltaStore` implementation, which this is based on
                filter_map: lfs_threshold_filtermap_fn(lfs_threshold),
                write_store: indexedlog,
            }) as BoxedWriteStore<Key, Entry>
        } else {
            indexedlog as BoxedWriteStore<Key, Entry>
        };
        Ok(maybe_filtered)
    }

    fn use_edenapi(&self) -> Result<bool> {
        Ok(self
            .config
            .get_or_default::<bool>("remotefilelog", "http")?)
    }

    fn build_edenapi(&self) -> Result<Arc<EdenApiAdapter<Client>>> {
        let reponame = get_repo_name(self.config)?;
        let extstored_policy = self.get_extstored_policy()?;

        Ok(Arc::new(EdenApiAdapter {
            client: Builder::from_config(self.config)?.build()?,
            repo: reponame,
            extstored_policy,
        }))
    }

    fn build_indexedlog(&self) -> Result<Arc<IndexedLogHgIdDataStore>> {
        let extstored_policy = self.get_extstored_policy()?;

        let cache_path = &get_cache_path(self.config, &self.suffix)?;
        Ok(Arc::new(IndexedLogHgIdDataStore::new(
            get_indexedlogdatastore_path(&cache_path)?,
            extstored_policy,
            self.config,
            IndexedLogDataStoreType::Shared,
        )?))
    }

    /// Return an Arc<IndexedLogHgIdDataStore> for another datastore to use and use this
    /// same IndexedLog object internally, so that both datastores share the same in-memory
    /// cache and will immediately see each other's writes reflected.
    pub fn build_shared_indexedlog(&mut self) -> Result<Arc<IndexedLogHgIdDataStore>> {
        let indexedlog = self.build_indexedlog()?;
        self.shared_indexedlog = Some(indexedlog.clone());
        Ok(indexedlog)
    }

    pub fn build(self) -> Result<BoxedReadStore<Key, StoreFile>> {
        // If this check wasn't just run by ContentStore, run it.
        // TODO(meyer): As written, without legacy fallback it'll happen twice, once in trees and once in files.
        if self.legacy_datastore.is_none() {
            let cache_path = get_cache_path(self.config, &self.suffix)?;
            check_cache_buster(&self.config, &cache_path);
        }

        let indexedlog = if let Some(shared_indexedlog) = self.shared_indexedlog.as_ref() {
            shared_indexedlog.clone()
        } else {
            self.build_indexedlog()?
        };
        let filtered_indexedlog = self.filtered_indexedlog(indexedlog.clone())?;

        let edenapi = if self.use_edenapi()? {
            if let Some(shared_edenapi) = self.shared_edenapi.as_ref() {
                // TOD(meyer): Same unnecessary ExtStoredPolicy issue here as above
                Some(
                    Arc::new(shared_edenapi.get_scmstore_adapter(self.get_extstored_policy()?))
                        as BoxedReadStore<Key, FileEntry>,
                )
            } else {
                Some(self.build_edenapi()? as BoxedReadStore<Key, FileEntry>)
            }
        } else {
            None
        };

        // TODO(meyer): Address combinatorial explosion here & decide what configurations should actually be supported.
        Ok(match (edenapi, self.legacy_datastore) {
            (Some(edenapi), Some(legacy)) => {
                let legacy_fallback = Arc::new(Fallback {
                    preferred: edenapi,
                    fallback: Arc::new(legacy) as BoxedReadStore<Key, Entry>,
                });

                Arc::new(FallbackCache {
                    preferred: indexedlog,
                    fallback: legacy_fallback as BoxedReadStore<Key, Entry>,
                    write_store: Some(filtered_indexedlog),
                })
            }
            (None, Some(legacy)) => Arc::new(FallbackCache {
                preferred: indexedlog,
                fallback: Arc::new(legacy) as BoxedReadStore<Key, Entry>,
                write_store: Some(filtered_indexedlog),
            }),
            (Some(edenapi), None) => Arc::new(FallbackCache {
                preferred: indexedlog,
                fallback: edenapi,
                write_store: Some(filtered_indexedlog),
            }),
            // TODO(meyer): Strongly type this. Should the client be able to easily identify EdenApi construction errors vs. others, etc?
            _ => bail!("Unsupported FileScmStoreBuilder Configuration"),
        })
    }
}
