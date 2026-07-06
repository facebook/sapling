/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use acl_manifest::RootAclManifestId;
use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use async_trait::async_trait;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use blobstore::StoreLoadable;
use context::CoreContext;
use derived_data::prefetch_content_metadata;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_manager::dependencies;
use derived_data_service_if as thrift;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future;
use futures::stream;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgManifestId;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;

use crate::derive_hg_augmented_manifest::subtree_copy_source_changesets;
use crate::mapping::RootHgAugmentedManifestId;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootHgAugmentedManifestV2Id(HgAugmentedManifestId);

impl RootHgAugmentedManifestV2Id {
    pub fn hg_augmented_manifest_id(&self) -> HgAugmentedManifestId {
        self.0
    }

    fn from_v1(root: RootHgAugmentedManifestId) -> Self {
        Self(root.hg_augmented_manifest_id())
    }

    fn into_v1(self) -> RootHgAugmentedManifestId {
        RootHgAugmentedManifestId::new(self.0)
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

async fn get_subtree_source_aug_roots(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
    known_aug_roots: Option<&HashMap<ChangesetId, RootHgAugmentedManifestV2Id>>,
) -> Result<HashMap<ChangesetId, HgAugmentedManifestId>> {
    let mut sources = HashMap::new();
    let mut missing_sources = Vec::new();

    for from_cs_id in subtree_copy_source_changesets(bonsai) {
        if let Some(aug) = known_aug_roots.and_then(|m| m.get(&from_cs_id)) {
            sources.insert(from_cs_id, aug.hg_augmented_manifest_id());
        } else {
            missing_sources.push(from_cs_id);
        }
    }

    if !missing_sources.is_empty() {
        let fetched_sources = derivation_ctx
            .fetch_derived_batch::<RootHgAugmentedManifestV2Id>(ctx, missing_sources.clone())
            .await?;
        for from_cs_id in missing_sources {
            if let Some(aug) = fetched_sources.get(&from_cs_id) {
                sources.insert(from_cs_id, aug.hg_augmented_manifest_id());
            } else {
                bail!(
                    "Subtree copy source augmented manifest for changeset {from_cs_id} not found; \
                     it must be derived before the changeset that copies from it",
                );
            }
        }
    }

    Ok(sources)
}

async fn lookup_mapped_root_hg_manifest_ids(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    csids: Vec<ChangesetId>,
) -> Result<HashMap<ChangesetId, HgManifestId>> {
    if csids.is_empty() {
        return Ok(HashMap::new());
    }

    let mappings = derivation_ctx
        .bonsai_hg_mapping()?
        .get(ctx, csids.into())
        .await?;
    let blobstore = Arc::clone(derivation_ctx.blobstore());

    stream::iter(mappings)
        .map(|entry| {
            let blobstore = Arc::clone(&blobstore);
            async move {
                let hg_manifest_id = entry
                    .hg_cs_id
                    .load(ctx, &blobstore)
                    .await
                    .with_context(|| {
                        format!(
                            "failed loading mapped HgChangeset {} for {}",
                            entry.hg_cs_id, entry.bcs_id,
                        )
                    })?
                    .manifestid();
                Ok((entry.bcs_id, hg_manifest_id))
            }
        })
        .buffer_unordered(100)
        .try_collect()
        .await
}

async fn lookup_mapped_root_hg_manifest_id(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    csid: ChangesetId,
) -> Result<Option<HgManifestId>> {
    Ok(
        lookup_mapped_root_hg_manifest_ids(ctx, derivation_ctx, vec![csid])
            .await?
            .remove(&csid),
    )
}

async fn derive_from_mapped_hg_manifest(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
    hg_manifest_id: HgManifestId,
    aug_parents: Vec<HgAugmentedManifestId>,
) -> Result<HgAugmentedManifestId> {
    let blobstore = derivation_ctx.blobstore();
    let csid = bonsai.get_changeset_id();
    let content_ids = bonsai
        .file_changes()
        .filter_map(|(_path, change)| change.simplify().map(|change| change.content_id()))
        .collect::<HashSet<_>>();
    let content_metadata_fut = prefetch_content_metadata(ctx, blobstore, content_ids);
    let acl_root_fut = derivation_ctx.fetch_dependency::<RootAclManifestId>(ctx, csid);
    let (content_metadata, acl_root) = future::try_join(content_metadata_fut, acl_root_fut).await?;
    let acl_root_overlay = crate::derive_hg_augmented_manifest::normalize_acl_root(&acl_root)?;

    crate::derive_hg_augmented_manifest::derive_from_hg_manifest_and_parents(
        ctx,
        blobstore,
        hg_manifest_id,
        aug_parents,
        &content_metadata,
        &derivation_ctx.restricted_paths(),
        acl_root_overlay,
    )
    .await
}

async fn derive_direct(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
    aug_parents: Vec<HgAugmentedManifestId>,
    known_aug_roots: Option<&HashMap<ChangesetId, RootHgAugmentedManifestV2Id>>,
) -> Result<HgAugmentedManifestId> {
    let blobstore = derivation_ctx.blobstore();
    let csid = bonsai.get_changeset_id();

    let (acl_root, source_aug_roots) = future::try_join(
        derivation_ctx.fetch_dependency::<RootAclManifestId>(ctx, csid),
        get_subtree_source_aug_roots(ctx, derivation_ctx, bonsai, known_aug_roots),
    )
    .await?;

    let acl_root_overlay = crate::derive_hg_augmented_manifest::normalize_acl_root(&acl_root)?;

    crate::derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai_changeset(
        ctx,
        blobstore,
        bonsai,
        aug_parents,
        &source_aug_roots,
        &derivation_ctx.restricted_paths(),
        acl_root_overlay,
    )
    .await
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
        known: Option<&HashMap<ChangesetId, Self>>,
    ) -> Result<Self> {
        let csid = bonsai.get_changeset_id();
        let aug_parents = parents
            .into_iter()
            .map(|p| p.hg_augmented_manifest_id())
            .collect();
        let root = if let Some(hg_manifest_id) =
            lookup_mapped_root_hg_manifest_id(ctx, derivation_ctx, csid).await?
        {
            derive_from_mapped_hg_manifest(
                ctx,
                derivation_ctx,
                &bonsai,
                hg_manifest_id,
                aug_parents,
            )
            .await?
        } else {
            derive_direct(ctx, derivation_ctx, &bonsai, aug_parents, known).await?
        };
        Ok(Self(root))
    }

    async fn derive_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
    ) -> Result<HashMap<ChangesetId, Self>> {
        let mut res: HashMap<ChangesetId, Self> = HashMap::new();
        let mapped_hg_manifest_ids = lookup_mapped_root_hg_manifest_ids(
            ctx,
            derivation_ctx,
            bonsais
                .iter()
                .map(BonsaiChangeset::get_changeset_id)
                .collect(),
        )
        .await?;
        for bonsai in &bonsais {
            let csid = bonsai.get_changeset_id();
            let aug_parents: Vec<_> = derivation_ctx
                .fetch_unknown_parents::<Self>(ctx, Some(&res), bonsai)
                .await?
                .into_iter()
                .map(|p| p.hg_augmented_manifest_id())
                .collect();
            let root = if let Some(hg_manifest_id) = mapped_hg_manifest_ids.get(&csid).copied() {
                derive_from_mapped_hg_manifest(
                    ctx,
                    derivation_ctx,
                    bonsai,
                    hg_manifest_id,
                    aug_parents,
                )
                .await?
            } else {
                derive_direct(ctx, derivation_ctx, bonsai, aug_parents, Some(&res)).await?
            };
            res.insert(csid, Self(root));
        }
        Ok(res)
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        self.into_v1()
            .store_mapping(ctx, derivation_ctx, changeset_id)
            .await
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        Ok(
            RootHgAugmentedManifestId::fetch(ctx, derivation_ctx, changeset_id)
                .await?
                .map(Self::from_v1),
        )
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
