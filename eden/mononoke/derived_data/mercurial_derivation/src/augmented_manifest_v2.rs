/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use acl_manifest::RootAclManifestId;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use blobstore::KeyedBlobstore;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_manager::dependencies;
use derived_data_service_if as thrift;
use mercurial_types::HgAugmentedManifestId;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;

use crate::augmented_derivation::AugmentedManifestRoot;
use crate::augmented_derivation::derive_batch_roots;
use crate::augmented_derivation::derive_from_mapped_hg_manifest;
use crate::augmented_derivation::derive_single_root;
use crate::augmented_derivation::lookup_mapped_root_hg_manifest_id;
use crate::mapping::RootHgAugmentedManifestId;

impl AugmentedManifestRoot for RootHgAugmentedManifestV2Id {
    fn from_root_id(root: HgAugmentedManifestId) -> Self {
        Self(root)
    }

    fn root_id(&self) -> HgAugmentedManifestId {
        self.0
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootHgAugmentedManifestV2Id(HgAugmentedManifestId);

impl RootHgAugmentedManifestV2Id {
    pub(crate) fn new(hg_augmented_manifest_id: HgAugmentedManifestId) -> Self {
        Self(hg_augmented_manifest_id)
    }

    pub fn hg_augmented_manifest_id(&self) -> HgAugmentedManifestId {
        self.0
    }

    /// Derives the mapped-Hg source choice from caller-provided parent content.
    /// This does not fetch or store V2 completion for those parents.
    pub async fn derive_from_mapped_hg_with_parents(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: &BonsaiChangeset,
        aug_parents: Vec<HgAugmentedManifestId>,
    ) -> Result<Option<Self>> {
        let Some(hg_manifest_id) =
            lookup_mapped_root_hg_manifest_id(ctx, derivation_ctx, bonsai.get_changeset_id())
                .await?
        else {
            return Ok(None);
        };
        Ok(Some(Self(
            derive_from_mapped_hg_manifest(
                ctx,
                derivation_ctx,
                bonsai,
                hg_manifest_id,
                aug_parents,
            )
            .await?,
        )))
    }

    fn from_v1(root: RootHgAugmentedManifestId) -> Self {
        Self(root.hg_augmented_manifest_id())
    }

    fn into_v1(self) -> RootHgAugmentedManifestId {
        RootHgAugmentedManifestId::new(self.0)
    }

    pub(crate) async fn fetch_private_mapping(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        let key = format_key(derivation_ctx, changeset_id);
        derivation_ctx
            .blobstore()
            .get(ctx, &key)
            .await?
            .map(TryInto::try_into)
            .transpose()
    }

    pub(crate) async fn store_mapping_with_publication(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
        publish_shared: bool,
    ) -> Result<()> {
        if publish_shared {
            self.into_v1()
                .store_mapping(ctx, derivation_ctx, changeset_id)
                .await
        } else {
            let key = format_key(derivation_ctx, changeset_id);
            derivation_ctx
                .blobstore()
                .put(ctx, key, self.into())
                .await?;
            Ok(())
        }
    }
}

impl TryFrom<BlobstoreBytes> for RootHgAugmentedManifestV2Id {
    type Error = anyhow::Error;

    fn try_from(blob: BlobstoreBytes) -> Result<Self> {
        RootHgAugmentedManifestId::try_from(blob).map(Self::from_v1)
    }
}

impl TryFrom<BlobstoreGetData> for RootHgAugmentedManifestV2Id {
    type Error = anyhow::Error;

    fn try_from(blob_get_data: BlobstoreGetData) -> Result<Self> {
        RootHgAugmentedManifestId::try_from(blob_get_data).map(Self::from_v1)
    }
}

impl From<RootHgAugmentedManifestV2Id> for BlobstoreBytes {
    fn from(root_hg_augmented_manifest_id: RootHgAugmentedManifestV2Id) -> Self {
        BlobstoreBytes::from(root_hg_augmented_manifest_id.into_v1())
    }
}

fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_hgaugmentedmanifest_v2.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootHgAugmentedManifestId>();
    format!("{root_prefix}{key_prefix}{changeset_id}")
}

pub(crate) fn should_publish_shared_mapping(repo_name: &str) -> bool {
    justknobs::eval(
        "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
        None,
        Some(RootHgAugmentedManifestV2Id::NAME),
    ) && justknobs::eval(
        "scm/mononoke:hg_augmented_manifests_v2_publish_shared_mapping",
        None,
        Some(repo_name),
    )
}

#[async_trait]
impl BonsaiDerivable for RootHgAugmentedManifestV2Id {
    const VARIANT: DerivableType = DerivableType::HgAugmentedManifestsV2;

    type Dependencies = dependencies![RootAclManifestId];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
        _known: Option<&HashMap<ChangesetId, Self>>,
    ) -> Result<Self> {
        derive_single_root(ctx, derivation_ctx, bonsai, parents).await
    }

    async fn derive_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
    ) -> Result<HashMap<ChangesetId, Self>> {
        derive_batch_roots(ctx, derivation_ctx, bonsais).await
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        let publish_shared = should_publish_shared_mapping(derivation_ctx.repo_name());
        self.store_mapping_with_publication(ctx, derivation_ctx, changeset_id, publish_shared)
            .await
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        if should_publish_shared_mapping(derivation_ctx.repo_name()) {
            Ok(
                RootHgAugmentedManifestId::fetch(ctx, derivation_ctx, changeset_id)
                    .await?
                    .map(Self::from_v1),
            )
        } else {
            // Shared roots are compatible migration boundaries, but only the
            // private mapping proves that V2 produced a root while shadowing.
            match Self::fetch_private_mapping(ctx, derivation_ctx, changeset_id).await? {
                Some(root) => Ok(Some(root)),
                None => Ok(
                    RootHgAugmentedManifestId::fetch(ctx, derivation_ctx, changeset_id)
                        .await?
                        .map(Self::from_v1),
                ),
            }
        }
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        crate::mapping::hg_augmented_manifest_id_from_derived_data(data, Self::NAME).map(Self)
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(crate::mapping::hg_augmented_manifest_id_into_derived_data(
            data.0,
        ))
    }
}
