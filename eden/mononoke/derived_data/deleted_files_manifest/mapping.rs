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
use derived_data::{BonsaiDerived, BonsaiDerivedMapping};
use futures::stream::{FuturesUnordered, TryStreamExt};
use mononoke_types::{BlobstoreBytes, BonsaiChangeset, ChangesetId, DeletedManifestId};
use repo_blobstore::RepoBlobstore;
use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto},
};

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
impl BonsaiDerived for RootDeletedManifestId {
    const NAME: &'static str = "deleted_manifest";
    type Mapping = RootDeletedManifestMapping;

    fn mapping(_ctx: &CoreContext, repo: &BlobRepo) -> Self::Mapping {
        RootDeletedManifestMapping::new(repo.blobstore().clone())
    }

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

impl RootDeletedManifestMapping {
    pub fn new(blobstore: RepoBlobstore) -> Self {
        Self { blobstore }
    }

    fn format_key(&self, cs_id: ChangesetId) -> String {
        format!("derived_root_deleted_manifest.{}", cs_id)
    }

    async fn fetch_deleted_manifest<'a>(
        &'a self,
        ctx: &'a CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<(ChangesetId, RootDeletedManifestId)>, Error> {
        let maybe_bytes = self.blobstore.get(ctx, &self.format_key(cs_id)).await?;
        match maybe_bytes {
            Some(bytes) => Ok(Some((cs_id, bytes.try_into()?))),
            None => Ok(None),
        }
    }
}

#[async_trait]
impl BonsaiDerivedMapping for RootDeletedManifestMapping {
    type Value = RootDeletedManifestId;

    async fn get(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::Value>, Error> {
        csids
            .into_iter()
            .map(|cs_id| self.fetch_deleted_manifest(&ctx, cs_id))
            .collect::<FuturesUnordered<_>>()
            .try_filter_map(|maybe_metadata| async move { Ok(maybe_metadata) })
            .try_collect()
            .await
    }

    async fn put(&self, ctx: CoreContext, csid: ChangesetId, id: Self::Value) -> Result<(), Error> {
        self.blobstore
            .put(&ctx, self.format_key(csid), id.into())
            .await
    }
}
