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
use blobstore::StoreLoadable;
use context::CoreContext;
use derived_data::prefetch_content_metadata;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivationContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future;
use futures::stream;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgManifestId;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;

use crate::mapping::RootHgAugmentedManifestId;

/// The canonical and rollout augmented-manifest types run the same derivation
/// and differ only in where they store and fetch roots.
pub(crate) trait AugmentedManifestRoot: BonsaiDerivable {
    fn from_root_id(root: HgAugmentedManifestId) -> Self;
    fn root_id(&self) -> HgAugmentedManifestId;
}

impl AugmentedManifestRoot for RootHgAugmentedManifestId {
    fn from_root_id(root: HgAugmentedManifestId) -> Self {
        Self::new(root)
    }

    fn root_id(&self) -> HgAugmentedManifestId {
        self.hg_augmented_manifest_id()
    }
}

pub(crate) async fn fetch_augmented_roots_with_fallback<Root: AugmentedManifestRoot>(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    source_csids: Vec<ChangesetId>,
    mut roots: HashMap<ChangesetId, HgAugmentedManifestId>,
    source_description: &str,
) -> Result<HashMap<ChangesetId, HgAugmentedManifestId>> {
    let missing_sources: Vec<_> = source_csids
        .iter()
        .copied()
        .filter(|csid| !roots.contains_key(csid))
        .collect();
    if !missing_sources.is_empty() {
        roots.extend(
            derivation_ctx
                .fetch_derived_batch::<Root>(ctx, missing_sources)
                .await?
                .into_iter()
                .map(|(csid, root)| (csid, root.root_id())),
        );
    }
    if let Some(csid) = source_csids.iter().find(|csid| !roots.contains_key(csid)) {
        bail!(
            "{source_description} augmented manifest for changeset {csid} not found; \
             it must be derived before the changeset that copies from it",
        );
    }
    Ok(roots)
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

pub(crate) async fn lookup_mapped_root_hg_manifest_id(
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

pub(crate) async fn derive_from_mapped_hg_manifest(
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
) -> Result<HgAugmentedManifestId> {
    let blobstore = derivation_ctx.blobstore();
    let csid = bonsai.get_changeset_id();

    let acl_root = derivation_ctx
        .fetch_dependency::<RootAclManifestId>(ctx, csid)
        .await?;

    let acl_root_overlay = crate::derive_hg_augmented_manifest::normalize_acl_root(&acl_root)?;

    crate::derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai_changeset(
        ctx,
        blobstore,
        bonsai,
        aug_parents,
        &derivation_ctx.restricted_paths(),
        acl_root_overlay,
    )
    .await
}

/// A commit with a Bonsai-to-Hg mapping builds from its mapped Hg manifest,
/// because an uploaded Hg changeset can differ from the one Mononoke would
/// derive, and the augmented manifest must match the Hg tree clients have.
pub(crate) async fn derive_single_root<Root: AugmentedManifestRoot>(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: BonsaiChangeset,
    parents: Vec<Root>,
) -> Result<Root> {
    let csid = bonsai.get_changeset_id();
    let aug_parents = parents.iter().map(Root::root_id).collect();
    let root = if let Some(hg_manifest_id) =
        lookup_mapped_root_hg_manifest_id(ctx, derivation_ctx, csid).await?
    {
        derive_from_mapped_hg_manifest(ctx, derivation_ctx, &bonsai, hg_manifest_id, aug_parents)
            .await?
    } else {
        derive_direct(ctx, derivation_ctx, &bonsai, aug_parents).await?
    };
    Ok(Root::from_root_id(root))
}

pub(crate) async fn derive_batch_roots<Root: AugmentedManifestRoot>(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsais: Vec<BonsaiChangeset>,
) -> Result<HashMap<ChangesetId, Root>> {
    let mut res: HashMap<ChangesetId, Root> = HashMap::new();
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
            .fetch_unknown_parents::<Root>(ctx, Some(&res), bonsai)
            .await?
            .iter()
            .map(Root::root_id)
            .collect();
        let root = if let Some(hg_manifest_id) = mapped_hg_manifest_ids.get(&csid).copied() {
            derive_from_mapped_hg_manifest(ctx, derivation_ctx, bonsai, hg_manifest_id, aug_parents)
                .await?
        } else {
            derive_direct(ctx, derivation_ctx, bonsai, aug_parents).await?
        };
        res.insert(csid, Root::from_root_id(root));
    }
    Ok(res)
}
