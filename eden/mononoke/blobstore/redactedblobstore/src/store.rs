/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::RedactionConfigBlobstore;
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
use mononoke_types::typed_hash::RedactionKeyListId;
use mononoke_types::RedactionKeyList;
use mononoke_types::Timestamp;
use redaction_set::RedactionSets;
use reloader::Loader;
use reloader::Reloader;
use sql::queries;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Clone)]
pub struct SqlRedactedContentStore {
    read_connection: Connection,
    write_connection: Connection,
}

queries! {

    write InsertRedactedBlobs(
        values: (content_key: String, task: String, add_timestamp: Timestamp, log_only: bool)
    ) {
        none,
        mysql(
            "INSERT INTO censored_contents(content_key, task, add_timestamp, log_only) VALUES {values}
            ON DUPLICATE KEY UPDATE task = VALUES(task), add_timestamp = VALUES(add_timestamp), log_ONLY = VALUES(log_only)
            "
        )
        sqlite(
            "REPLACE INTO censored_contents(content_key, task, add_timestamp, log_only) VALUES {values}"
        )
    }

    read GetAllRedactedBlobs() -> (String, String, Option<bool>) {
        "SELECT content_key, task, log_only
        FROM censored_contents"
    }

    write DeleteRedactedBlobs(>list content_keys: String) {
        none,
        "DELETE FROM censored_contents
         WHERE content_key IN {content_keys}"
    }
}

impl SqlConstruct for SqlRedactedContentStore {
    const LABEL: &'static str = "censored_contents";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-redacted.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            write_connection: connections.write_connection,
            read_connection: connections.read_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlRedactedContentStore {}

#[derive(Clone, Debug)]
pub struct RedactedMetadata {
    pub task: String,
    pub log_only: bool,
}

#[derive(Debug, Clone)]
pub enum RedactedBlobs {
    FromSql(Arc<HashMap<String, RedactedMetadata>>),
    FromConfigerator(Arc<ConfigeratorRedactedBlobs>),
}

impl RedactedBlobs {
    pub fn redacted(&self) -> Arc<HashMap<String, RedactedMetadata>> {
        match self {
            Self::FromSql(hm) => hm.clone(),
            Self::FromConfigerator(conf) => conf.get_map(),
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

impl SqlRedactedContentStore {
    pub async fn get_all_redacted_blobs(&self) -> Result<RedactedBlobs> {
        let redacted_blobs = GetAllRedactedBlobs::query(&self.read_connection).await?;
        Ok(RedactedBlobs::FromSql(Arc::new(
            redacted_blobs
                .into_iter()
                .map(|(key, task, log_only)| {
                    let redacted_metadata = RedactedMetadata {
                        task,
                        log_only: log_only.unwrap_or(false),
                    };
                    (key, redacted_metadata)
                })
                .collect(),
        )))
    }

    pub async fn insert_redacted_blobs(
        &self,
        content_keys: &[String],
        task: &String,
        add_timestamp: &Timestamp,
        log_only: bool,
    ) -> Result<()> {
        let log_only = &log_only;
        let redacted_inserts: Vec<_> = content_keys
            .iter()
            .map(move |key| (key, task, add_timestamp, log_only))
            .collect();

        InsertRedactedBlobs::query(&self.write_connection, &redacted_inserts[..])
            .await
            .map_err(Error::from)
            .map(|_| ())
    }

    pub async fn delete_redacted_blobs(&self, content_keys: &[String]) -> Result<()> {
        DeleteRedactedBlobs::query(&self.write_connection, content_keys)
            .await
            .map_err(Error::from)
            .map(|_| ())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[fbinit::test]
    async fn test_redacted_store(_fb: fbinit::FacebookInit) {
        let key_a = "aaaaaaaaaaaaaaaaaaaa".to_string();
        let key_b = "bbbbbbbbbbbbbbbbbbbb".to_string();
        let key_c = "cccccccccccccccccccc".to_string();
        let key_d = "dddddddddddddddddddd".to_string();
        let task1 = "task1".to_string();
        let task2 = "task2".to_string();
        let redacted_keys1 = vec![key_a.clone(), key_b.clone()];
        let redacted_keys2 = vec![key_c.clone(), key_d.clone()];

        let store = SqlRedactedContentStore::with_sqlite_in_memory().unwrap();

        store
            .insert_redacted_blobs(&redacted_keys1, &task1, &Timestamp::now(), false)
            .await
            .expect("insert failed");
        store
            .insert_redacted_blobs(&redacted_keys2, &task2, &Timestamp::now(), true)
            .await
            .expect("insert failed");

        let all = store.get_all_redacted_blobs().await.expect("select failed");
        let res = all.redacted();
        assert_eq!(res.len(), 4);
        assert!(!res.get(&key_a).unwrap().log_only);
        assert!(!res.get(&key_b).unwrap().log_only);
        assert!(res.get(&key_c).unwrap().log_only);
        assert!(res.get(&key_d).unwrap().log_only);

        store
            .insert_redacted_blobs(&redacted_keys1, &task1, &Timestamp::now(), true)
            .await
            .expect("insert failed");
        let all = store.get_all_redacted_blobs().await.expect("select failed");
        let res = all.redacted();
        assert!(res.get(&key_a).unwrap().log_only);
        assert!(res.get(&key_b).unwrap().log_only);

        store
            .delete_redacted_blobs(&redacted_keys1)
            .await
            .expect("delete failed");
        let all = store.get_all_redacted_blobs().await.expect("select failed");
        let res = all.redacted();

        assert!(res.contains_key(&key_c));
        assert!(res.contains_key(&key_d));
        assert_eq!(res.len(), 2);
    }
}
