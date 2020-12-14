/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::derive::{derive_deleted_files_manifest, get_changes};
use anyhow::{Error, Result};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::{Blobstore, BlobstoreGetData};
use bytes::Bytes;
use context::CoreContext;
use derived_data::{impl_bonsai_derived_mapping, BlobstoreRootIdMapping, BonsaiDerivable};
use mononoke_types::{BlobstoreBytes, BonsaiChangeset, DeletedManifestId};
use repo_blobstore::RepoBlobstore;
use std::convert::{TryFrom, TryInto};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootDeletedManifestId(DeletedManifestId);

impl RootDeletedManifestId {
    pub fn deleted_manifest_id(&self) -> &DeletedManifestId {
        &self.0
    }
}

impl TryFrom<BlobstoreBytes> for RootDeletedManifestId {
    type Error = Error;
    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        DeletedManifestId::from_bytes(&blob_bytes.into_bytes()).map(RootDeletedManifestId)
    }
}

impl TryFrom<BlobstoreGetData> for RootDeletedManifestId {
    type Error = Error;
    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootDeletedManifestId> for BlobstoreBytes {
    fn from(root_mf_id: RootDeletedManifestId) -> Self {
        BlobstoreBytes::from_bytes(Bytes::copy_from_slice(root_mf_id.0.blake2().as_ref()))
    }
}

#[async_trait]
impl BonsaiDerivable for RootDeletedManifestId {
    const NAME: &'static str = "deleted_manifest";


    async fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self, Error> {
        let bcs_id = bonsai.get_changeset_id();
        let changes = get_changes(&ctx, &repo, bonsai).await?;
        let id = derive_deleted_files_manifest(
            ctx,
            repo,
            bcs_id,
            parents
                .into_iter()
                .map(|root_mf_id| root_mf_id.deleted_manifest_id().clone())
                .collect(),
            changes,
        )
        .await?;
        Ok(RootDeletedManifestId(id))
    }
}

#[derive(Clone)]
pub struct RootDeletedManifestMapping {
    blobstore: RepoBlobstore,
}

#[async_trait]
impl BlobstoreRootIdMapping for RootDeletedManifestMapping {
    type Value = RootDeletedManifestId;

    fn new(repo: &BlobRepo) -> Result<Self> {
        Ok(Self {
            blobstore: repo.get_blobstore(),
        })
    }

    fn blobstore(&self) -> &dyn Blobstore {
        &self.blobstore
    }

    fn prefix(&self) -> &'static str {
        "derived_root_deleted_manifest."
    }
}

impl_bonsai_derived_mapping!(
    RootDeletedManifestMapping,
    BlobstoreRootIdMapping,
    RootDeletedManifestId
);
