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
use blobstore::LoadableError;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use context::CoreContext;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::Blob;
use mononoke_types::BlobstoreKey;
use mononoke_types::BlobstoreValue;
use mononoke_types::RedactionKeyList;
use mononoke_types::typed_hash::RedactionKeyListId;
use redaction_set::RedactionSets;
use reloader::Loader;
use reloader::Reloader;
use tracing::debug;

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
            .with_context(|| format!("redaction sets not found at {config_path}"))?;
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
            Reloader::reload_periodically(ctx, || std::time::Duration::from_mins(1), loader)
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
        debug!("Reloading redacted config from configerator");
        let map: HashMap<String, RedactedMetadata> = stream::iter(
            config
                .all_redactions
                .iter()
                .map(|redaction| async move {
                    let keylist_id = RedactionKeyListId::from_str(&redaction.id)
                        .with_context(|| format!("Invalid keylist id: {}", redaction.id))?;
                    let keylist = load(ctx, keylist_id, &blobstore)
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

pub async fn load<'a, B: Blobstore>(
    ctx: &'a CoreContext,
    keylist_id: RedactionKeyListId,
    blobstore: &'a B,
) -> Result<RedactionKeyList, LoadableError> {
    let blobstore_key = keylist_id.blobstore_key();
    let get = blobstore.get(ctx, &blobstore_key);

    let bytes = get
        .await?
        .ok_or(LoadableError::Missing(blobstore_key.clone()))?;

    let blob: Blob<RedactionKeyListId> = Blob::new(keylist_id, bytes.into_raw_bytes());

    <RedactionKeyList as BlobstoreValue>::from_blob(blob).map_err(LoadableError::Error)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use blobstore::Blobstore;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use memblob::Memblob;
    use mononoke_macros::mononoke;
    use mononoke_types::BlobstoreKey;
    use mononoke_types::BlobstoreValue;
    use mononoke_types::RedactionKeyList;
    use redaction_set::RedactionSet;
    use redaction_set::RedactionSets;
    use scuba_ext::MononokeScubaSampleBuilder;

    use super::*;
    use crate::RedactedBlobstore;
    use crate::RedactedBlobstoreConfig;

    async fn store_keylist(
        ctx: &CoreContext,
        bs: &RedactionConfigBlobstore,
        keys: &[&str],
    ) -> Result<String> {
        let blob = RedactionKeyList {
            keys: keys.iter().map(|k| k.to_string()).collect(),
        }
        .into_blob();
        let id = *blob.id();
        bs.put(ctx, id.blobstore_key(), blob.into()).await?;
        Ok(id.to_string())
    }

    async fn log_only_for(
        ctx: &CoreContext,
        bs: &RedactionConfigBlobstore,
        sets: Vec<RedactionSet>,
        key: &str,
    ) -> Result<bool> {
        let config = Arc::new(RedactionSets {
            all_redactions: sets,
        });
        let inner = InnerConfig::new(config, ctx, bs).await?;
        Ok(inner.map.get(key).expect("key must be redacted").log_only)
    }

    /// FIXME: BUG! Two redaction sets that share a key merged last-writer-wins in key-list
    /// completion order, so an enforced redaction could be downgraded to log-only (access allowed)
    /// depending on fetch order. A key enforced by any set must be enforced whatever the order. This
    /// test pins the current order-dependent behaviour; the next diff fixes it.
    #[mononoke::fbinit_test]
    async fn overlapping_redaction_sets_merge(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let bs = RedactionConfigBlobstore::new(Arc::new(Memblob::default()) as Arc<dyn Blobstore>);

        // Two key lists that both contain "k".
        let id_enforced = store_keylist(&ctx, &bs, &["k"]).await?;
        let id_log_only = store_keylist(&ctx, &bs, &["k", "other"]).await?;

        let enforced = RedactionSet {
            reason: "T1 enforced".to_string(),
            id: id_enforced,
            enforce: true,
        };
        let log_only = RedactionSet {
            reason: "T2 log only".to_string(),
            id: id_log_only,
            enforce: false,
        };

        // Enforced set listed first, log-only second (with an in-memory blobstore, key lists
        // complete in list order, so the log-only entry is collected last and wins).
        let a = log_only_for(&ctx, &bs, vec![enforced.clone(), log_only.clone()], "k").await?;
        // Same two sets, other order.
        let b = log_only_for(&ctx, &bs, vec![log_only.clone(), enforced.clone()], "k").await?;

        // FIXME: BUG! "k" is enforced by T1, so it must be enforced (log_only == false) whatever the
        // order. Today the result depends on the order the key lists are fetched.
        assert_eq!((a, b), (true, false), "merge is order dependent today");

        // And the observable effect: with order (enforced, log_only) a get of the redacted key is
        // ALLOWED.
        let config = Arc::new(RedactionSets {
            all_redactions: vec![enforced, log_only],
        });
        let inner = InnerConfig::new(config, &ctx, &bs).await?;
        let redacted = RedactedBlobs::FromHashMapForTests(inner.map.clone());
        let blob = RedactedBlobstore::new(
            Memblob::default(),
            RedactedBlobstoreConfig::new(
                Some(Arc::new(redacted)),
                MononokeScubaSampleBuilder::with_discard(),
            ),
        );
        // FIXME: BUG! should be Err(RedactionError::Redacted)
        assert!(blob.get(&ctx, "k").await.is_ok());
        Ok(())
    }
}
