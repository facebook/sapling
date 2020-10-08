/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::{format_err, Context, Result};

use dag::InProcessIdDag;

use blobstore::{Blobstore, BlobstoreBytes};
use context::CoreContext;
use mononoke_types::RepositoryId;

use crate::dag::Dag;
use crate::types::IdDagVersion;

pub struct IdDagSaveStore {
    repo_id: RepositoryId,
    blobstore: Arc<dyn Blobstore>,
}

impl IdDagSaveStore {
    pub fn new(repo_id: RepositoryId, blobstore: Arc<dyn Blobstore>) -> Self {
        Self { repo_id, blobstore }
    }

    pub async fn find(
        &self,
        ctx: &CoreContext,
        iddag_version: IdDagVersion,
    ) -> Result<Option<InProcessIdDag>> {
        let bytes_opt = self
            .blobstore
            .get(ctx.clone(), self.key(iddag_version))
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
        let dag: InProcessIdDag = mincode::deserialize(&bytes.into_raw_bytes())?;
        Ok(Some(dag))
    }

    pub async fn load(
        &self,
        ctx: &CoreContext,
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

    pub async fn save(
        &self,
        ctx: &CoreContext,
        iddag_version: IdDagVersion,
        iddag: &InProcessIdDag,
    ) -> Result<()> {
        let buffer = mincode::serialize(iddag)?;
        self.blobstore
            .put(
                ctx.clone(),
                self.key(iddag_version),
                BlobstoreBytes::from_bytes(buffer),
            )
            .await
    }

    pub async fn save_from_dag(
        &self,
        ctx: &CoreContext,
        iddag_version: IdDagVersion,
        dag: &Dag,
    ) -> Result<()> {
        self.save(ctx, iddag_version, &dag.iddag).await
    }

    fn key(&self, iddag_version: IdDagVersion) -> String {
        format!(
            "segmented_changelog.iddag_save.v1.{}.{}",
            self.repo_id, iddag_version.0
        )
    }
}
