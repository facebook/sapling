/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

use blobstore::Blobstore;
use context::CoreContext;
use filestore::BlobCopier;
use filestore::FetchKey;
use filestore::FilestoreConfig;
use mononoke_types::ContentMetadata;
use mononoke_types::RepositoryId;
use repo_blobstore::RepoBlobstore;

use crate::Bubble;
use crate::BubbleId;

struct Copier {
    /// Blobstore that writes to ephemeral blobstore but has no bubble prefix
    /// and doesn't fallback to persistent blobstore
    raw_eph_blobstore: Arc<dyn Blobstore>,
    repo_prefix: String,
    prefix_bubble1: String,
    prefix_bubble2: String,
}

#[async_trait]
impl BlobCopier for Copier {
    async fn copy(&self, ctx: &CoreContext, key: String) -> Result<()> {
        let old_key = [
            self.prefix_bubble1.as_str(),
            self.repo_prefix.as_str(),
            &key,
        ]
        .concat();
        let new_key = [
            self.prefix_bubble2.as_str(),
            self.repo_prefix.as_str(),
            &key,
        ]
        .concat();
        self.raw_eph_blobstore.copy(ctx, &old_key, new_key).await?;
        Ok(())
    }
}

impl Bubble {
    pub async fn copy_file_to_bubble(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        repo_blobstore: RepoBlobstore,
        other: BubbleId,
        config: FilestoreConfig,
        id: FetchKey,
    ) -> Result<Option<ContentMetadata>> {
        self.check_unexpired()?;
        let blobstore = self.wrap_repo_blobstore(repo_blobstore);
        let data = match filestore::get_metadata(&blobstore, ctx, &id).await? {
            Some(data) => data,
            None => return Ok(None),
        };

        let raw_eph_blobstore = Arc::new(self.blobstore.clone().into_inner()) as Arc<dyn Blobstore>;

        filestore::copy(
            blobstore,
            &Copier {
                raw_eph_blobstore,
                repo_prefix: repo_id.prefix(),
                prefix_bubble1: self.bubble_id().prefix(),
                prefix_bubble2: other.prefix(),
            },
            config,
            ctx,
            &data,
        )
        .await?;
        Ok(Some(data))
    }
}
