/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use context::CoreContext;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::RedactionKeyList;
use mononoke_types::typed_hash::RedactionKeyListId;
use redaction_set::RedactionSets;
use reloader::Loader;
use reloader::Reloader;

use crate::RedactionConfigBlobstore;

#[derive(Clone, Debug)]
pub struct RedactedMetadata {
    pub task: String,
    pub log_only: bool,
}

#[derive(Debug, Clone)]
pub enum RedactedBlobs {
    FromConfigerator(Arc<ConfigeratorRedactedBlobs>),
    FromHashMapForTests(Arc<HashMap<String, RedactedMetadata>>),
}

impl RedactedBlobs {
    pub fn redacted(&self) -> Arc<HashMap<String, RedactedMetadata>> {
        match self {
            Self::FromConfigerator(conf) => conf.get_map(),
            Self::FromHashMapForTests(hash_map) => hash_map.clone(),
        }
    }

    pub async fn from_configerator(
        store: &ConfigStore,
        config_path: &str,
        ctx: CoreContext,
        blobstore: Arc<RedactionConfigBlobstore>,
    ) -> Result<Self, Error> {
        let handle = store
            .get_config_handle(config_path.to_string())
            .with_context(|| format!("redaction sets not found at {}", config_path))?;
        Ok(Self::FromConfigerator(Arc::new(
            ConfigeratorRedactedBlobs::new(ctx, handle, blobstore).await?,
        )))
    }
}

#[derive(Debug)]
struct InnerConfig {
    #[allow(dead_code)]
    raw_config: Arc<RedactionSets>,
    map: Arc<HashMap<String, RedactedMetadata>>,
}

#[derive(Debug)]
pub struct ConfigeratorRedactedBlobs(Reloader<InnerConfig>);

struct InnerConfigLoader {
    ctx: CoreContext,
    handle: ConfigHandle<RedactionSets>,
    blobstore: Arc<RedactionConfigBlobstore>,
    last_config: Option<Arc<RedactionSets>>,
}
impl InnerConfigLoader {
    fn new(
        ctx: CoreContext,
        handle: ConfigHandle<RedactionSets>,
        blobstore: Arc<RedactionConfigBlobstore>,
    ) -> Self {
        Self {
            ctx,
            handle,
            blobstore,
            last_config: None,
        }
    }
}

#[async_trait]
impl Loader<InnerConfig> for InnerConfigLoader {
    async fn load(&mut self) -> Result<Option<InnerConfig>> {
        let new_config = self.handle.get();
        if match &self.last_config {
            Some(old) => !Arc::ptr_eq(old, &new_config) && *old != new_config,
            None => true,
        } {
            let res = Some(InnerConfig::new(new_config.clone(), &self.ctx, &self.blobstore).await?);
            self.last_config = Some(new_config);
            Ok(res)
        } else {
            Ok(None)
        }
    }
}

impl ConfigeratorRedactedBlobs {
    async fn new(
        ctx: CoreContext,
        handle: ConfigHandle<RedactionSets>,
        blobstore: Arc<RedactionConfigBlobstore>,
    ) -> Result<Self> {
        let loader = InnerConfigLoader::new(ctx.clone(), handle, blobstore);
        let reloader =
            Reloader::reload_periodically(ctx, || std::time::Duration::from_secs(60), loader)
                .await?;

        Ok(ConfigeratorRedactedBlobs(reloader))
    }

    fn get_map(&self) -> Arc<HashMap<String, RedactedMetadata>> {
        self.0.load().map.clone()
    }
}

impl InnerConfig {
    async fn new(
        config: Arc<RedactionSets>,
        ctx: &CoreContext,
        blobstore: &dyn Blobstore,
    ) -> Result<Self> {
        slog::debug!(ctx.logger(), "Reloading redacted config from configerator");
        let map: HashMap<String, RedactedMetadata> = stream::iter(
            config
                .all_redactions
                .iter()
                .map(|redaction| async move {
                    let keylist: RedactionKeyList = RedactionKeyListId::from_str(&redaction.id)
                        .with_context(|| format!("Invalid keylist id: {}", redaction.id))?
                        .load(ctx, &blobstore)
                        .await
                        .with_context(|| format!("Keylist with id {} not found", redaction.id))?;
                    let keys_with_metadata = keylist
                        .keys
                        .into_iter()
                        .map(|key| {
                            (
                                key,
                                RedactedMetadata {
                                    task: redaction.reason.clone(),
                                    log_only: !redaction.enforce,
                                },
                            )
                        })
                        .collect::<Vec<_>>();

                    Result::<_, Error>::Ok(keys_with_metadata)
                })
                // If we don't collect, it triggers a compile bug P421135476
                .collect::<Vec<_>>(),
        )
        .buffer_unordered(100)
        .try_collect::<Vec<Vec<_>>>()
        .await?
        .into_iter()
        .flatten()
        .collect();
        Ok(Self {
            raw_config: config,
            map: Arc::new(map),
        })
    }
}
