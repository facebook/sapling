/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::format_err;
use anyhow::Context;
use anyhow::Result;

use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use context::CoreContext;
use mononoke_types::RepositoryId;

use crate::logging::log_new_iddag_version;
use crate::types::IdDagVersion;
use crate::InProcessIdDag;

pub struct IdDagSaveStore {
    repo_id: RepositoryId,
    blobstore: Arc<dyn Blobstore>,
}

impl IdDagSaveStore {
    pub fn new(repo_id: RepositoryId, blobstore: Arc<dyn Blobstore>) -> Self {
        Self { repo_id, blobstore }
    }

    pub async fn find<'a>(
        &'a self,
        ctx: &'a CoreContext,
        iddag_version: IdDagVersion,
    ) -> Result<Option<InProcessIdDag>> {
        let bytes_opt = self
            .blobstore
            .get(ctx, &self.key(iddag_version))
            .await
            .with_context(|| {
                format!(
                    "loading prebuilt segmented changelog iddag version {}",
                    iddag_version.0
                )
            })?;
        let bytes = match bytes_opt {
            None => return Ok(None),
            Some(b) => b,
        };

        let deserialization = move || {
            let iddag: InProcessIdDag = mincode::deserialize(&bytes.into_raw_bytes())?;
            anyhow::Ok(iddag)
        };
        // In tests we can't offload deserialization to spawn blocking because
        // of a tokio bug where in time::pause environment the time
        // auto-advancement won't wait for spawned blocking tasks.
        let iddag = if cfg!(test) {
            deserialization()?
        } else {
            tokio::task::spawn_blocking(deserialization).await??
        };

        Ok(Some(iddag))
    }

    pub async fn load<'a>(
        &'a self,
        ctx: &'a CoreContext,
        iddag_version: IdDagVersion,
    ) -> Result<InProcessIdDag> {
        self.find(ctx, iddag_version).await?.ok_or_else(|| {
            format_err!(
                "Not Found: prebuilt iddag (repo_id: {}, version: {})",
                self.repo_id,
                iddag_version.0,
            )
        })
    }

    pub async fn save<'a>(
        &'a self,
        ctx: &'a CoreContext,
        iddag: &InProcessIdDag,
    ) -> Result<IdDagVersion> {
        let buffer = mincode::serialize(iddag)?;
        let iddag_version = IdDagVersion::from_serialized_bytes(&buffer);
        self.blobstore
            .put(
                ctx,
                self.key(iddag_version),
                BlobstoreBytes::from_bytes(buffer),
            )
            .await
            .context("saving iddag in blobstore")?;
        log_new_iddag_version(ctx, self.repo_id, iddag_version);
        Ok(iddag_version)
    }

    fn key(&self, iddag_version: IdDagVersion) -> String {
        format!("segmented_changelog_iddag.blake2.{}", iddag_version.0)
    }
}
