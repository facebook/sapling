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
use anyhow::anyhow;
use anyhow::bail;
use async_trait::async_trait;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use blobstore::KeyedBlobstore;
use blobstore::StoreLoadable;
use bonsai_hg_mapping::BonsaiHgMappingEntry;
use bytes::Bytes;
use context::CoreContext;
use derived_data::batch::DEFAULT_STACK_FILE_CHANGES_LIMIT;
use derived_data::batch::FileConflicts;
use derived_data::batch::SplitOptions;
use derived_data::batch::split_bonsais_in_linear_stacks;
use derived_data::prefetch_content_metadata;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivableUntopologically;
use derived_data_manager::DerivationContext;
use derived_data_manager::dependencies;
use futures::TryStreamExt;
use futures::future;
use futures::stream;
use futures::stream::StreamExt;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathOrPrefix;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgChangesetId;
use mercurial_types::HgNodeHash;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableUntopologicallyVariant;
use mononoke_types::FileChange;
use mononoke_types::RepoPath;
use restricted_paths_common::ArcRestrictedPathsConfigBased;
use restricted_paths_common::ManifestType;
use restricted_paths_common::RestrictedPathManifestIdEntry;
use stats::prelude::*;
use tracing::debug;
use tracing::warn;

define_stats! {
    prefix = "mononoke.derived_data.hgchangesets";
    new_parallel: timeseries(Rate, Sum),
}

use derived_data_service_if as thrift;

use crate::derive_hg_augmented_manifest::AclOverlayCache;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct MappedHgChangesetId(HgChangesetId);

impl MappedHgChangesetId {
    pub(crate) fn new(hg_changeset_id: HgChangesetId) -> Self {
        MappedHgChangesetId(hg_changeset_id)
    }

    pub fn hg_changeset_id(&self) -> HgChangesetId {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct HgChangesetDeriveOptions {
    pub set_committer_field: bool,
}

#[async_trait]
impl BonsaiDerivable for MappedHgChangesetId {
    const VARIANT: DerivableType = DerivableType::HgChangesets;

    type Dependencies = dependencies![];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
        _known: Option<&HashMap<ChangesetId, Self>>,
    ) -> Result<Self> {
        if bonsai.is_snapshot() {
            bail!("Can't derive Hg changeset for snapshot")
        }
        let subtree_change_sources =
            get_subtree_change_sources(ctx, derivation_ctx, &bonsai, &HashMap::new()).await?;
        let derivation_opts = get_hg_changeset_derivation_options(derivation_ctx);
        let (derived, _) = crate::derive_hg_changeset::derive_from_parents(
            ctx,
            derivation_ctx.blobstore(),
            bonsai,
            parents,
            subtree_change_sources,
            &derivation_opts,
            derivation_ctx.restricted_paths(),
        )
        .await?;
        Ok(derived)
    }

    async fn derive_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
    ) -> Result<HashMap<ChangesetId, Self>> {
        if bonsais.is_empty() {
            return Ok(HashMap::new());
        }
        if bonsais.iter().any(|bonsai| bonsai.is_snapshot()) {
            bail!("Can't derive Hg changeset for snapshot");
        }

        STATS::new_parallel.add_value(1);
        let linear_stacks = split_bonsais_in_linear_stacks(
            &bonsais,
            SplitOptions {
                file_conflicts: FileConflicts::ChangeDelete,
                copy_info: true,
                file_changes_limit: DEFAULT_STACK_FILE_CHANGES_LIMIT,
            },
        )?;
        let mut res: HashMap<ChangesetId, Self> = HashMap::new();
        let batch_len = bonsais.len();

        let derivation_opts = get_hg_changeset_derivation_options(derivation_ctx);

        let mut bonsais = bonsais;
        for stack in linear_stacks {
            let derived_parents = future::try_join_all(
                stack
                    .parents
                    .into_iter()
                    .map(|p| derivation_ctx.fetch_unknown_dependency::<Self>(ctx, Some(&res), p)),
            )
            .await?;
            if let Some(item) = stack.stack_items.first() {
                debug!(
                    "derive hgchangeset batch at {} (stack of {} from batch of {})",
                    item.cs_id.to_hex(),
                    stack.stack_items.len(),
                    batch_len,
                );
            }

            // after the line below `bonsais` will contain all the bonsais that we are
            // going to derive now, and `left_bonsais` will contain all the bonsais that
            // we are going to derive in the next step
            let left_bonsais = bonsais.split_off(stack.stack_items.len());
            if derived_parents.len() > 1 || bonsais.len() == 1 {
                // we can't derive stack for a merge commit or for a commit that contains renames,
                // or subtree changes so let's derive it without batching
                for bonsai in bonsais {
                    let parents = derivation_ctx
                        .fetch_unknown_parents(ctx, Some(&res), &bonsai)
                        .await?;
                    let cs_id = bonsai.get_changeset_id();
                    let subtree_change_sources =
                        get_subtree_change_sources(ctx, derivation_ctx, &bonsai, &res).await?;
                    let derivation_opts = get_hg_changeset_derivation_options(derivation_ctx);
                    let (derived, _) = crate::derive_hg_changeset::derive_from_parents(
                        ctx,
                        derivation_ctx.blobstore(),
                        bonsai,
                        parents,
                        subtree_change_sources,
                        &derivation_opts,
                        derivation_ctx.restricted_paths(),
                    )
                    .await?;
                    res.insert(cs_id, derived);
                }
            } else {
                let first = stack.stack_items.first().map(|item| item.cs_id);
                let last = stack.stack_items.last().map(|item| item.cs_id);
                let derived =
                    crate::derive_hg_changeset::derive_simple_hg_changeset_stack_without_copy_info(
                        ctx,
                        derivation_ctx.blobstore(),
                        bonsais,
                        derived_parents.first().cloned(),
                        &derivation_opts,
                        derivation_ctx.restricted_paths(),
                    )
                    .await
                    .with_context(|| format!("failed deriving stack of {first:?} to {last:?}"))?;

                // This pattern is used to convert a ref to tuple into a tuple of refs.
                #[allow(clippy::map_identity)]
                res.extend(derived.into_iter().map(|(csid, hg_cs_id)| (csid, hg_cs_id)));
            }
            bonsais = left_bonsais;
        }

        Ok(res)
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        derivation_ctx
            .bonsai_hg_mapping()?
            .add(
                ctx,
                BonsaiHgMappingEntry {
                    hg_cs_id: self.0,
                    bcs_id: changeset_id,
                },
            )
            .await?;
        Ok(())
    }

    async fn store_mapping_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        derived: Vec<(ChangesetId, Self)>,
    ) -> Result<()> {
        let entries: Vec<_> = derived
            .into_iter()
            .map(|(bcs_id, hg_cs_id)| BonsaiHgMappingEntry {
                hg_cs_id: hg_cs_id.0,
                bcs_id,
            })
            .collect();
        derivation_ctx
            .bonsai_hg_mapping()?
            .bulk_add(ctx, &entries)
            .await?;
        Ok(())
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        Ok(Self::fetch_batch(ctx, derivation_ctx, &[changeset_id])
            .await?
            .into_iter()
            .next()
            .map(|(_, hg_id)| hg_id))
    }

    async fn fetch_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_ids: &[ChangesetId],
    ) -> Result<HashMap<ChangesetId, Self>> {
        Ok(derivation_ctx
            .bonsai_hg_mapping()?
            .get(ctx, changeset_ids.to_vec().into())
            .await?
            .into_iter()
            .map(|entry| (entry.bcs_id, MappedHgChangesetId(entry.hg_cs_id)))
            .collect())
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::hg_changeset(
            thrift::DerivedDataHgChangeset::mapped_hgchangeset_id(id),
        ) = data
        {
            HgChangesetId::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME,
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::hg_changeset(
            thrift::DerivedDataHgChangeset::mapped_hgchangeset_id(data.0.into_thrift()),
        ))
    }
}

fn get_hg_changeset_derivation_options(
    derivation_ctx: &DerivationContext,
) -> HgChangesetDeriveOptions {
    HgChangesetDeriveOptions {
        set_committer_field: derivation_ctx.config().hg_set_committer_extra,
    }
}

/// Whether to derive augmented manifests directly from bonsai changesets
/// and parent augmented manifests, bypassing HgManifest construction.
fn should_use_direct_derivation(repo_name: &str) -> Result<bool> {
    Ok(justknobs::eval(
        "scm/mononoke:augmented_manifest_direct_derivation",
        None,
        Some(repo_name),
    ))
}

pub(crate) async fn get_subtree_change_sources(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
    mapping: &HashMap<ChangesetId, MappedHgChangesetId>,
) -> Result<HashMap<ChangesetId, HgChangesetId>> {
    let subtree_change_sources = bonsai
        .subtree_changes()
        .iter()
        .flat_map(|(_path, change)| change.change_source().map(|(csid, _)| csid))
        .collect::<HashSet<_>>();
    let mut sources = HashMap::new();
    let mut other = Vec::new();
    for source in subtree_change_sources {
        if let Some(hg_cs_id) = mapping.get(&source) {
            sources.insert(source, hg_cs_id.hg_changeset_id());
        } else {
            other.push(source);
        }
    }
    if !other.is_empty() {
        sources.extend(
            derivation_ctx
                .bonsai_hg_mapping()?
                .get(ctx, other.into())
                .await?
                .into_iter()
                .map(|entry| (entry.bcs_id, entry.hg_cs_id)),
        )
    };
    Ok(sources)
}

/// Asynchronously track HgAugmentedManifest IDs for all restricted paths in a commit.
///
/// This is used for derive_from_predecessor.  We don't know which restricted paths were changed
/// or introduced by this commit as the parent may not have been derived yet.  Instead, we log all
/// of them.  This is ok as derive_from_predecessor is only used for backfilling.
async fn track_all_restricted_paths(
    ctx: &CoreContext,
    restricted_paths: ArcRestrictedPathsConfigBased,
    hg_cs_id: HgChangesetId,
    root_hg_aug_mfid: HgAugmentedManifestId,
    blobstore: Arc<dyn KeyedBlobstore>,
) -> Result<()> {
    // Check if restricted paths tracking is enabled via justknobs
    let restricted_paths_enabled = justknobs::eval(
        "scm/mononoke:enabled_restricted_paths_access_logging",
        None, // hashing
        // Adding a switch value to be able to disable writes only
        Some("hg_augmented_manifest_write"),
    );

    if !restricted_paths_enabled {
        return Ok(());
    }

    // Get the configured restricted paths. Skip read-only roots: derivation must
    // not record new manifest-id-store entries for them.
    let restricted_dirs: Vec<PathOrPrefix> = restricted_paths
        .config()
        .path_restriction_metadata
        .iter()
        .filter(|(_, metadata)| !metadata.read_only)
        .map(|(non_root_mpath, _)| PathOrPrefix::Path(non_root_mpath.clone().into()))
        .collect();

    if restricted_dirs.is_empty() {
        return Ok(());
    }

    // Find all directory entries for the restricted paths and store their
    // HgAugmentedManifest IDs in the manifest id store
    let entries: Vec<RestrictedPathManifestIdEntry> = root_hg_aug_mfid
        .find_entries(ctx.clone(), blobstore, restricted_dirs)
        .try_filter_map(|(path, manif_entry)| async move {
            match manif_entry {
                Entry::Tree(hg_aug_manifest_id) => {
                    let repo_path = RepoPath::dir(path).context("Expected NonRootMPath")?;

                    let entry = RestrictedPathManifestIdEntry::new(
                        ManifestType::HgAugmented,
                        hg_aug_manifest_id.to_string().into(),
                        repo_path,
                    )?;
                    Ok(Some(entry))
                }
                Entry::Leaf(_) => bail!("Path {path} is not a directory"),
            }
        })
        .try_collect::<Vec<RestrictedPathManifestIdEntry>>()
        .await
        .context("Building restricted path manifest entries")?;

    if entries.is_empty() {
        return Ok(());
    }

    if let Err(e) = restricted_paths
        .manifest_id_store()
        .add_entries(ctx, &entries)
        .await
        .context("Failed to add entries to the manifest id store")
    {
        warn!("Failed to track restricted paths for changeset {hg_cs_id}: {e}");
    }

    Ok(())
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootHgAugmentedManifestId(HgAugmentedManifestId);

impl RootHgAugmentedManifestId {
    pub(crate) fn new(hg_augmented_manifest_id: HgAugmentedManifestId) -> Self {
        RootHgAugmentedManifestId(hg_augmented_manifest_id)
    }

    pub fn hg_augmented_manifest_id(&self) -> HgAugmentedManifestId {
        self.0
    }
}

impl TryFrom<BlobstoreBytes> for RootHgAugmentedManifestId {
    type Error = anyhow::Error;

    fn try_from(blob: BlobstoreBytes) -> Result<Self> {
        HgAugmentedManifestId::from_bytes(&blob.into_bytes()).map(Self)
    }
}

impl TryFrom<BlobstoreGetData> for RootHgAugmentedManifestId {
    type Error = anyhow::Error;

    fn try_from(blob_get_data: BlobstoreGetData) -> Result<Self> {
        blob_get_data.into_bytes().try_into()
    }
}

impl From<RootHgAugmentedManifestId> for BlobstoreBytes {
    fn from(root_hg_augmented_manifest_id: RootHgAugmentedManifestId) -> Self {
        BlobstoreBytes::from_bytes(Bytes::copy_from_slice(
            root_hg_augmented_manifest_id.0.as_bytes(),
        ))
    }
}

pub fn format_key(derivation_ctx: &DerivationContext, cs_id: ChangesetId) -> String {
    let root_prefix = "derived_root_hgaugmentedmanifest.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootHgAugmentedManifestId>();
    format!("{root_prefix}{key_prefix}{cs_id}")
}

/// Resolve subtree-copy source augmented manifest roots for a bonsai.
///
/// For each `SubtreeCopy` in the bonsai's `subtree_changes`, finds the
/// source changeset's `RootHgAugmentedManifestId`. Checks `known_aug_roots`
/// first (batch-local cache) then falls back to persisted mappings.
async fn get_subtree_source_aug_roots(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
    known_aug_roots: Option<&HashMap<ChangesetId, RootHgAugmentedManifestId>>,
) -> Result<HashMap<ChangesetId, mercurial_types::HgAugmentedManifestId>> {
    let mut sources = HashMap::new();
    let mut missing_sources = Vec::new();
    let mut seen_missing_sources = HashSet::new();

    for (_dest, change) in bonsai.subtree_changes() {
        let Some((from_cs_id, _from_path)) = change.copy_source() else {
            continue;
        };
        if sources.contains_key(&from_cs_id) || seen_missing_sources.contains(&from_cs_id) {
            continue;
        }
        if let Some(aug) = known_aug_roots.and_then(|m| m.get(&from_cs_id)) {
            sources.insert(from_cs_id, aug.hg_augmented_manifest_id());
        } else {
            missing_sources.push(from_cs_id);
            seen_missing_sources.insert(from_cs_id);
        }
    }

    if !missing_sources.is_empty() {
        let fetched_sources = derivation_ctx
            .fetch_derived_batch::<RootHgAugmentedManifestId>(ctx, missing_sources.clone())
            .await?;
        for from_cs_id in missing_sources {
            if let Some(aug) = fetched_sources.get(&from_cs_id) {
                sources.insert(from_cs_id, aug.hg_augmented_manifest_id());
            } else {
                anyhow::bail!(
                    "Subtree copy source augmented manifest for changeset {from_cs_id} not found; \
                     it must be derived before the changeset that copies from it",
                );
            }
        }
    }

    Ok(sources)
}

/// If this Bonsai changeset is already mapped to a Mercurial changeset, use
/// that changeset's root manifest id as the canonical augmented-manifest root.
/// Bonsai-native changesets with no Hg mapping use the directly-computed root.
async fn lookup_mapped_root_hg_node_ids(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    csids: Vec<ChangesetId>,
) -> Result<HashMap<ChangesetId, HgNodeHash>> {
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
                let expected_root = entry
                    .hg_cs_id
                    .load(ctx, &blobstore)
                    .await
                    .with_context(|| {
                        format!(
                            "failed loading mapped HgChangeset {} for {}",
                            entry.hg_cs_id, entry.bcs_id,
                        )
                    })?
                    .manifestid()
                    .into_nodehash();
                Ok((entry.bcs_id, expected_root))
            }
        })
        .buffer_unordered(100)
        .try_collect()
        .await
}

async fn lookup_mapped_root_hg_node_id(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    csid: ChangesetId,
) -> Result<Option<HgNodeHash>> {
    Ok(
        lookup_mapped_root_hg_node_ids(ctx, derivation_ctx, vec![csid])
            .await?
            .remove(&csid),
    )
}

/// Derive an augmented manifest directly from a bonsai changeset and parent
/// augmented manifests, without constructing HgManifests. Shared by
/// `derive_single` and `derive_batch`.
async fn derive_direct(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
    aug_parents: Vec<HgAugmentedManifestId>,
    known_aug_roots: Option<&HashMap<ChangesetId, RootHgAugmentedManifestId>>,
    expected_root: Option<HgNodeHash>,
    acl_overlay_cache: &mut AclOverlayCache,
) -> Result<HgAugmentedManifestId> {
    let blobstore = derivation_ctx.blobstore();
    let csid = bonsai.get_changeset_id();

    let mut content_ids = HashSet::new();
    let file_changes: Vec<_> = bonsai
        .file_changes()
        .map(|(path, fc)| {
            Ok((
                path.clone(),
                match fc {
                    FileChange::Change(tc) => {
                        content_ids.insert(tc.content_id());
                        Some(tc.clone())
                    }
                    FileChange::Deletion => None,
                    FileChange::UntrackedChange(_) | FileChange::UntrackedDeletion => {
                        bail!("Can't derive manifest for snapshot")
                    }
                },
            ))
        })
        .collect::<Result<_>>()?;

    let parent_csids = {
        let mut p = bonsai.parents();
        (p.next(), p.next())
    };

    let (content_metadata, acl_root) = future::try_join(
        prefetch_content_metadata(ctx, blobstore, content_ids),
        derivation_ctx.fetch_dependency::<RootAclManifestId>(ctx, csid),
    )
    .await?;

    // `derive_augmented_manifest_from_bonsai` builds the ACL overlay map itself,
    // scoped to the paths this changeset rebuilds (or a full walk for merges).
    let acl_root_overlay = crate::derive_hg_augmented_manifest::normalize_acl_root(&acl_root)?;

    let source_aug_roots =
        get_subtree_source_aug_roots(ctx, derivation_ctx, bonsai, known_aug_roots).await?;
    let subtree_replacements =
        crate::derive_hg_augmented_manifest::build_augmented_subtree_replacements(
            ctx,
            blobstore,
            bonsai,
            &source_aug_roots,
        )
        .await?;

    crate::derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        ctx,
        blobstore,
        aug_parents,
        file_changes,
        subtree_replacements,
        parent_csids,
        &content_metadata,
        expected_root,
        &derivation_ctx.restricted_paths(),
        acl_root_overlay,
        acl_overlay_cache,
    )
    .await
}

#[async_trait]
impl BonsaiDerivable for RootHgAugmentedManifestId {
    const VARIANT: DerivableType = DerivableType::HgAugmentedManifests;

    type Dependencies = dependencies![MappedHgChangesetId, RootAclManifestId];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
        _known: Option<&HashMap<ChangesetId, Self>>,
    ) -> Result<Self> {
        let csid = bonsai.get_changeset_id();

        if should_use_direct_derivation(derivation_ctx.repo_name())? {
            let aug_parents = parents
                .into_iter()
                .map(|p| p.hg_augmented_manifest_id())
                .collect();
            let expected_root = lookup_mapped_root_hg_node_id(ctx, derivation_ctx, csid).await?;
            let mut acl_overlay_cache = HashMap::new();
            let root = derive_direct(
                ctx,
                derivation_ctx,
                &bonsai,
                aug_parents,
                None,
                expected_root,
                &mut acl_overlay_cache,
            )
            .await?;
            return Ok(Self(root));
        }

        let blobstore = derivation_ctx.blobstore();
        let content_ids = bonsai
            .file_changes()
            .filter_map(|(_path, change)| change.simplify().map(|change| change.content_id()))
            .collect::<HashSet<_>>();
        let content_metadata_fut = prefetch_content_metadata(ctx, blobstore, content_ids);

        // Wrap the dependency fetch and envelope load into one future so the
        // manifest id is resolved concurrently with the content metadata and
        // acl_root fetches, matching the pre-cleanup concurrency.
        let hg_manifest_id_fut = async {
            let hg_cs = derivation_ctx
                .fetch_dependency::<MappedHgChangesetId>(ctx, csid)
                .await?;
            anyhow::Ok(
                hg_cs
                    .hg_changeset_id()
                    .load(ctx, blobstore)
                    .await?
                    .manifestid(),
            )
        };
        let acl_root_fut = derivation_ctx.fetch_dependency::<RootAclManifestId>(ctx, csid);

        let (hg_manifest_id, content_metadata, acl_root) =
            future::try_join3(hg_manifest_id_fut, content_metadata_fut, acl_root_fut).await?;

        let acl_root_overlay = crate::derive_hg_augmented_manifest::normalize_acl_root(&acl_root)?;

        let parents = parents
            .into_iter()
            .map(|parent| parent.hg_augmented_manifest_id())
            .collect();
        let root_hg_aug_mfid =
            crate::derive_hg_augmented_manifest::derive_from_hg_manifest_and_parents(
                ctx,
                blobstore,
                hg_manifest_id,
                parents,
                &content_metadata,
                &derivation_ctx.restricted_paths(),
                acl_root_overlay,
            )
            .await?;

        Ok(Self(root_hg_aug_mfid))
    }

    async fn derive_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
    ) -> Result<HashMap<ChangesetId, Self>> {
        let blobstore = derivation_ctx.blobstore();
        let mut res: HashMap<ChangesetId, Self> = HashMap::new();

        if should_use_direct_derivation(derivation_ctx.repo_name())? {
            let expected_roots = lookup_mapped_root_hg_node_ids(
                ctx,
                derivation_ctx,
                bonsais
                    .iter()
                    .map(BonsaiChangeset::get_changeset_id)
                    .collect(),
            )
            .await?;
            let mut acl_overlay_cache = HashMap::new();
            for bonsai in &bonsais {
                let csid = bonsai.get_changeset_id();
                let aug_parents: Vec<_> = derivation_ctx
                    .fetch_unknown_parents::<Self>(ctx, Some(&res), bonsai)
                    .await?
                    .into_iter()
                    .map(|p| p.hg_augmented_manifest_id())
                    .collect();
                let root = derive_direct(
                    ctx,
                    derivation_ctx,
                    bonsai,
                    aug_parents,
                    Some(&res),
                    expected_roots.get(&csid).copied(),
                    &mut acl_overlay_cache,
                )
                .await?;
                res.insert(csid, Self(root));
            }
            return Ok(res);
        }

        for bonsai in &bonsais {
            let csid = bonsai.get_changeset_id();
            let content_ids = bonsai
                .file_changes()
                .filter_map(|(_path, change)| change.simplify().map(|change| change.content_id()))
                .collect::<HashSet<_>>();
            let content_metadata_fut = prefetch_content_metadata(ctx, blobstore, content_ids);

            // Wrap the dependency fetch and envelope load into one future so
            // the manifest id is resolved concurrently with the content
            // metadata and acl_root fetches, matching the pre-cleanup
            // concurrency.
            let hg_manifest_id_fut = async {
                let hg_cs = derivation_ctx
                    .fetch_dependency::<MappedHgChangesetId>(ctx, csid)
                    .await?;
                anyhow::Ok(
                    hg_cs
                        .hg_changeset_id()
                        .load(ctx, blobstore)
                        .await?
                        .manifestid(),
                )
            };
            let acl_root_fut = derivation_ctx.fetch_dependency::<RootAclManifestId>(ctx, csid);

            let (hg_manifest_id, content_metadata, acl_root) =
                future::try_join3(hg_manifest_id_fut, content_metadata_fut, acl_root_fut).await?;

            let acl_root_overlay =
                crate::derive_hg_augmented_manifest::normalize_acl_root(&acl_root)?;

            let parents: Vec<_> = derivation_ctx
                .fetch_unknown_parents::<Self>(ctx, Some(&res), bonsai)
                .await?
                .into_iter()
                .map(|p| p.hg_augmented_manifest_id())
                .collect();

            let root = crate::derive_hg_augmented_manifest::derive_from_hg_manifest_and_parents(
                ctx,
                blobstore,
                hg_manifest_id,
                parents,
                &content_metadata,
                &derivation_ctx.restricted_paths(),
                acl_root_overlay,
            )
            .await?;

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
        let key = format_key(derivation_ctx, changeset_id);
        derivation_ctx
            .blobstore()
            .put(ctx, key, self.into())
            .await?;
        Ok(())
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        let key = format_key(derivation_ctx, changeset_id);
        Ok(derivation_ctx
            .blobstore()
            .get(ctx, &key)
            .await?
            .map(TryInto::try_into)
            .transpose()?)
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::hg_augmented_manifest(
            thrift::DerivedDataHgAugmentedManifest::root_hg_augmented_manifest_id(id),
        ) = data
        {
            HgAugmentedManifestId::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME,
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::hg_augmented_manifest(
            thrift::DerivedDataHgAugmentedManifest::root_hg_augmented_manifest_id(
                data.0.into_thrift(),
            ),
        ))
    }
}

#[async_trait]
impl DerivableUntopologically for RootHgAugmentedManifestId {
    const DERIVABLE_UNTOPOLOGICALLY_VARIANT: DerivableUntopologicallyVariant =
        DerivableUntopologicallyVariant::HgAugmentedManifests;
    type PredecessorDependencies = dependencies![MappedHgChangesetId, RootAclManifestId];

    async fn unsafe_derive_untopologically(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
    ) -> Result<Self> {
        let csid = bonsai.get_changeset_id();
        let (hg_changeset_id, acl_root) = future::try_join(
            async {
                Ok(derivation_ctx
                    .fetch_dependency::<MappedHgChangesetId>(ctx, csid)
                    .await?
                    .hg_changeset_id())
            },
            derivation_ctx.fetch_dependency::<RootAclManifestId>(ctx, csid),
        )
        .await?;
        let acl_root_overlay = crate::derive_hg_augmented_manifest::normalize_acl_root(&acl_root)?;
        let hg_manifest_id = hg_changeset_id
            .load(ctx, derivation_ctx.blobstore())
            .await?
            .manifestid();
        let root = crate::derive_hg_augmented_manifest::derive_from_full_hg_manifest(
            ctx.clone(),
            Arc::clone(derivation_ctx.blobstore()),
            hg_manifest_id,
            acl_root_overlay,
        )
        .await?;

        // Track restricted paths for the derived HgAugmentedManifest
        track_all_restricted_paths(
            ctx,
            derivation_ctx.restricted_paths(),
            hg_changeset_id,
            root,
            Arc::clone(derivation_ctx.blobstore()),
        )
        .await?;

        Ok(Self(root))
    }
}

#[cfg(test)]
mod test {
    use blobstore::Loadable;
    use bonsai_hg_mapping::BonsaiHgMapping;
    use bookmarks::BookmarkKey;
    use bookmarks::Bookmarks;
    use borrowed::borrowed;
    use changesets_creation::save_changesets;
    use cloned::cloned;
    use commit_graph::CommitGraph;
    use commit_graph::CommitGraphRef;
    use commit_graph::CommitGraphWriter;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use fixtures::BranchEven;
    use fixtures::BranchUneven;
    use fixtures::BranchWide;
    use fixtures::Linear;
    use fixtures::ManyDiamonds;
    use fixtures::ManyFilesDirs;
    use fixtures::MergeEven;
    use fixtures::MergeUneven;
    use fixtures::TestRepoFixture;
    use fixtures::UnsharedMergeEven;
    use fixtures::UnsharedMergeUneven;
    use futures::Future;
    use futures::FutureExt;
    use futures::TryStreamExt;
    use justknobs::test_helpers::JustKnobsInMemory;
    use justknobs::test_helpers::KnobVal;
    use justknobs::test_helpers::with_just_knobs_async;
    use mononoke_macros::mononoke;
    use mononoke_types::MPath;
    use mononoke_types::SubtreeChange;
    use repo_blobstore::RepoBlobstore;
    use repo_blobstore::RepoBlobstoreRef;
    use repo_derived_data::RepoDerivedData;
    use repo_derived_data::RepoDerivedDataRef;
    use repo_identity::RepoIdentity;
    use tests_utils::CreateCommitContext;

    use super::*;
    use crate::DeriveHgChangeset;

    #[derive(Clone)]
    #[facet::container]
    struct TestRepo {
        #[facet]
        bonsai_hg_mapping: dyn BonsaiHgMapping,
        #[facet]
        bookmarks: dyn Bookmarks,
        #[facet]
        repo_blobstore: RepoBlobstore,
        #[facet]
        repo_derived_data: RepoDerivedData,
        #[facet]
        filestore_config: FilestoreConfig,
        #[facet]
        commit_graph: CommitGraph,
        #[facet]
        commit_graph_writer: dyn CommitGraphWriter,
        #[facet]
        repo_identity: RepoIdentity,
    }

    async fn all_commits_descendants_to_ancestors(
        ctx: CoreContext,
        repo: TestRepo,
    ) -> Result<Vec<(ChangesetId, HgChangesetId)>> {
        let master_book = BookmarkKey::new("master").unwrap();
        let bcs_id = repo
            .bookmarks
            .get(ctx.clone(), &master_book, bookmarks::Freshness::MostRecent)
            .await?
            .ok_or_else(|| anyhow!("Missing master bookmark"))?;

        repo.commit_graph()
            .ancestors_difference_stream(&ctx, vec![bcs_id], vec![])
            .await?
            .and_then(move |new_bcs_id| {
                cloned!(ctx, repo);
                async move {
                    let hg_cs_id = repo.derive_hg_changeset(&ctx, new_bcs_id).await?;
                    Result::<_>::Ok((new_bcs_id, hg_cs_id))
                }
            })
            .try_collect()
            .await
    }

    async fn verify_repo<F, Fut>(fb: FacebookInit, repo_func: F) -> Result<()>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = TestRepo>,
    {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = repo_func().await;
        println!("Processing {}", repo.repo_identity.name());
        borrowed!(ctx, repo);

        let commits_desc_to_anc =
            all_commits_descendants_to_ancestors(ctx.clone(), repo.clone()).await?;

        // Recreate repo from scratch and derive everything again
        let repo = repo_func().await;
        let csids = commits_desc_to_anc
            .clone()
            .into_iter()
            .rev()
            .map(|(cs_id, _)| cs_id)
            .collect::<Vec<_>>();
        let manager = repo.repo_derived_data().manager();

        manager
            .derive_exactly_batch::<MappedHgChangesetId>(ctx, csids.clone(), None)
            .await?;
        let batch_derived = manager
            .fetch_derived_batch::<MappedHgChangesetId>(ctx, csids, None)
            .await?;

        for (cs_id, hg_cs_id) in commits_desc_to_anc.into_iter().rev() {
            println!("{} {} {:?}", cs_id, hg_cs_id, batch_derived.get(&cs_id));
            assert_eq!(batch_derived.get(&cs_id).map(|x| x.0), Some(hg_cs_id));
        }

        Ok(())
    }

    async fn save_bonsai_with_subtree_changes(
        ctx: &CoreContext,
        repo: &TestRepo,
        parents: Vec<ChangesetId>,
        subtree_changes: Vec<(MPath, SubtreeChange)>,
    ) -> Result<ChangesetId> {
        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([
                (
                    "scm/mononoke:enable_subtree_changes".to_string(),
                    KnobVal::Bool(true),
                ),
                (
                    "scm/mononoke:enable_manifest_altering_subtree_changes".to_string(),
                    KnobVal::Bool(true),
                ),
            ])),
            async move {
                let mut bonsai = CreateCommitContext::new(ctx, repo, parents)
                    .set_message("subtree change commit")
                    .create_commit_object()
                    .await?;
                bonsai.subtree_changes = subtree_changes.into_iter().collect();
                let bonsai = bonsai.freeze()?;
                let cs_id = bonsai.get_changeset_id();
                save_changesets(ctx, repo, vec![bonsai]).await?;
                Ok(cs_id)
            }
            .boxed(),
        )
        .await
    }

    fn subtree_copy(
        to_path: &str,
        from_path: &str,
        from_cs_id: ChangesetId,
    ) -> Result<(MPath, SubtreeChange)> {
        Ok((
            MPath::new(to_path)?,
            SubtreeChange::copy(MPath::new(from_path)?, from_cs_id),
        ))
    }

    #[mononoke::fbinit_test]
    async fn test_direct_derive_single_uses_mapped_hg_root(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

        // Given: a child commit whose dependencies and parent augmented
        // manifest are already derived.
        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("a.txt", "initial")
            .commit()
            .await?;
        let child = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("a.txt", "modified")
            .commit()
            .await?;
        let csids = vec![root, child];
        let manager = repo.repo_derived_data().manager();
        manager
            .derive_exactly_batch::<MappedHgChangesetId>(&ctx, csids.clone(), None)
            .await?;
        manager
            .derive_exactly_batch::<RootAclManifestId>(&ctx, csids.clone(), None)
            .await?;
        manager
            .derive_exactly_batch::<RootHgAugmentedManifestId>(&ctx, vec![root], None)
            .await?;
        let parent_aug = manager
            .fetch_derived::<RootHgAugmentedManifestId>(&ctx, root, None)
            .await?
            .context("Missing RootHgAugmentedManifestId for parent")?;
        let bonsai = Loadable::load(&child, &ctx, &repo.repo_blobstore).await?;
        let derivation_ctx = manager.derivation_context(None);

        // When: invoking the direct derive_single implementation with the JK
        // enabled.
        let derived = with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([(
                "scm/mononoke:augmented_manifest_direct_derivation".to_string(),
                KnobVal::Bool(true),
            )])),
            async {
                RootHgAugmentedManifestId::derive_single(
                    &ctx,
                    &derivation_ctx,
                    bonsai,
                    vec![parent_aug],
                    None,
                )
                .await
            }
            .boxed(),
        )
        .await?;

        // Then: derive_single returns a loadable augmented manifest using the
        // canonical Hg root from the existing Bonsai-to-Hg mapping.
        let aug_envelope = Loadable::load(
            &derived.hg_augmented_manifest_id(),
            &ctx,
            &repo.repo_blobstore,
        )
        .await?;
        let mapped_child = manager
            .fetch_derived::<MappedHgChangesetId>(&ctx, child, None)
            .await?
            .context("Missing MappedHgChangesetId for child")?;
        let expected_root =
            Loadable::load(&mapped_child.hg_changeset_id(), &ctx, &repo.repo_blobstore)
                .await?
                .manifestid()
                .into_nodehash();
        assert_eq!(
            aug_envelope.augmented_manifest.hg_node_id, expected_root,
            "direct derive_single should use the mapped Hg root",
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_direct_derivation_jk_falls_back_for_subtree_changes(
        fb: FacebookInit,
    ) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

        // Given: a commit with manifest-altering subtree changes and all
        // dependencies required by the legacy augmented-manifest path.
        let source = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("src/a.txt", "copied content")
            .commit()
            .await?;
        let child = save_bonsai_with_subtree_changes(
            &ctx,
            &repo,
            vec![source],
            vec![subtree_copy("dst", "src", source)?],
        )
        .await?;
        let csids = vec![source, child];
        let manager = repo.repo_derived_data().manager();
        manager
            .derive_exactly_batch::<MappedHgChangesetId>(&ctx, csids.clone(), None)
            .await?;
        manager
            .derive_exactly_batch::<RootAclManifestId>(&ctx, csids.clone(), None)
            .await?;

        // When: deriving through the manager with direct derivation enabled.
        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([(
                "scm/mononoke:augmented_manifest_direct_derivation".to_string(),
                KnobVal::Bool(true),
            )])),
            async {
                manager
                    .derive_exactly_batch::<RootHgAugmentedManifestId>(&ctx, csids.clone(), None)
                    .await
            }
            .boxed(),
        )
        .await?;

        // Then: the subtree-copy result is present and matches the canonical
        // Hg manifest, showing that the JK-gated direct path fell back to the
        // legacy subtree-aware path.
        let child_aug = manager
            .fetch_derived::<RootHgAugmentedManifestId>(&ctx, child, None)
            .await?
            .context("Missing RootHgAugmentedManifestId for subtree child")?
            .hg_augmented_manifest_id();
        let mapped_child = manager
            .fetch_derived::<MappedHgChangesetId>(&ctx, child, None)
            .await?
            .context("Missing MappedHgChangesetId for subtree child")?;
        let expected_hg_manifest =
            Loadable::load(&mapped_child.hg_changeset_id(), &ctx, &repo.repo_blobstore)
                .await?
                .manifestid();
        let copied_path = MPath::new("dst/a.txt")?;
        let aug_leaf = child_aug
            .find_entry(
                ctx.clone(),
                repo.repo_blobstore.clone(),
                copied_path.clone(),
            )
            .await?
            .and_then(Entry::into_leaf)
            .context("subtree copy should appear in the augmented manifest")?;
        let hg_leaf = expected_hg_manifest
            .find_entry(ctx.clone(), repo.repo_blobstore.clone(), copied_path)
            .await?
            .and_then(Entry::into_leaf)
            .context("subtree copy should appear in the Hg manifest")?;
        assert_eq!(aug_leaf.file_type, hg_leaf.0);
        assert_eq!(aug_leaf.filenode, hg_leaf.1.into_nodehash());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_direct_derive_batch_via_manager_persists_blobs(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

        // Given: a linear stack whose dependencies are derived, so the manager
        // can invoke the direct augmented-manifest path under its write-batched
        // derivation context.
        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("a.txt", "initial")
            .commit()
            .await?;
        let child = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("a.txt", "modified")
            .commit()
            .await?;
        let csids = vec![root, child];
        let manager = repo.repo_derived_data().manager();
        manager
            .derive_exactly_batch::<MappedHgChangesetId>(&ctx, csids.clone(), None)
            .await?;
        manager
            .derive_exactly_batch::<RootAclManifestId>(&ctx, csids.clone(), None)
            .await?;

        // When: deriving augmented manifests through the manager with direct
        // derivation enabled.
        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([(
                "scm/mononoke:augmented_manifest_direct_derivation".to_string(),
                KnobVal::Bool(true),
            )])),
            async {
                manager
                    .derive_exactly_batch::<RootHgAugmentedManifestId>(&ctx, csids.clone(), None)
                    .await
            }
            .boxed(),
        )
        .await?;

        let derived = manager
            .fetch_derived_batch::<RootHgAugmentedManifestId>(&ctx, csids.clone(), None)
            .await?;
        let mapped = manager
            .fetch_derived_batch::<MappedHgChangesetId>(&ctx, csids.clone(), None)
            .await?;

        // Then: mappings point at augmented-manifest blobs that are loadable
        // from the repo blobstore after the manager-owned flush.
        for cs_id in &csids {
            let aug = derived
                .get(cs_id)
                .with_context(|| format!("Missing RootHgAugmentedManifestId for {cs_id}"))?;
            let aug_envelope =
                Loadable::load(&aug.hg_augmented_manifest_id(), &ctx, &repo.repo_blobstore).await?;
            let hg_cs_id = mapped
                .get(cs_id)
                .with_context(|| format!("Missing MappedHgChangesetId for {cs_id}"))?
                .hg_changeset_id();
            let expected_root = Loadable::load(&hg_cs_id, &ctx, &repo.repo_blobstore)
                .await?
                .manifestid()
                .into_nodehash();
            assert_eq!(
                aug_envelope.augmented_manifest.hg_node_id, expected_root,
                "direct derivation through the manager should persist the canonical root for {cs_id}",
            );
        }

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_direct_derive_batch_without_hgchangeset_mapping(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

        // Given: a linear stack with only the non-HgChangeset dependency
        // prederived. MappedHgChangesetId is still a manager dependency in this
        // stack, so call derive_batch directly below the manager dependency gate.
        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("a.txt", "initial")
            .commit()
            .await?;
        let child = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("a.txt", "modified")
            .commit()
            .await?;
        let grandchild = CreateCommitContext::new(&ctx, &repo, vec![child])
            .add_file("b.txt", "new file")
            .commit()
            .await?;
        let csids = vec![root, child, grandchild];
        let manager = repo.repo_derived_data().manager();
        manager
            .derive_exactly_batch::<RootAclManifestId>(&ctx, csids.clone(), None)
            .await?;
        for cs_id in &csids {
            assert!(
                manager
                    .fetch_derived::<MappedHgChangesetId>(&ctx, *cs_id, None)
                    .await?
                    .is_none(),
                "fixture should not prederive MappedHgChangesetId for {cs_id}",
            );
        }

        let mut bonsais = Vec::new();
        for cs_id in &csids {
            bonsais.push(Loadable::load(cs_id, &ctx, &repo.repo_blobstore).await?);
        }
        let derivation_ctx = manager.derivation_context(None);

        // When: invoking the direct derive_batch implementation with the JK
        // enabled, bypassing manager dependency validation.
        let derived = with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([(
                "scm/mononoke:augmented_manifest_direct_derivation".to_string(),
                KnobVal::Bool(true),
            )])),
            async { RootHgAugmentedManifestId::derive_batch(&ctx, &derivation_ctx, bonsais).await }
                .boxed(),
        )
        .await?;

        // Then: the direct path produces content-derived roots and does not
        // create HgChangeset mappings as a side effect.
        for cs_id in &csids {
            let aug = derived
                .get(cs_id)
                .with_context(|| format!("Missing RootHgAugmentedManifestId for {cs_id}"))?;
            let aug_envelope =
                Loadable::load(&aug.hg_augmented_manifest_id(), &ctx, &repo.repo_blobstore).await?;
            assert_eq!(
                aug_envelope.augmented_manifest.hg_node_id,
                aug_envelope.augmented_manifest.computed_node_id,
                "no-Hg direct derivation should use content-derived root for {cs_id}",
            );
            assert!(
                manager
                    .fetch_derived::<MappedHgChangesetId>(&ctx, *cs_id, None)
                    .await?
                    .is_none(),
                "direct derivation must not create MappedHgChangesetId for {cs_id}",
            );
        }

        Ok(())
    }

    /// Like `verify_repo`, but derives `RootHgAugmentedManifestId`.
    /// When the skip-writes knob is active, this exercises the
    /// write-skipping path and verifies that HgManifest blobs can
    /// still be read through the reconstruction layer.
    async fn verify_repo_aug<F, Fut>(fb: FacebookInit, repo_func: F) -> Result<()>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = TestRepo>,
    {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = repo_func().await;
        println!("Processing {} (augmented)", repo.repo_identity.name());
        borrowed!(ctx, repo);

        let commits_desc_to_anc =
            all_commits_descendants_to_ancestors(ctx.clone(), repo.clone()).await?;

        // Recreate repo from scratch and derive augmented manifests.
        let repo = repo_func().await;
        let csids = commits_desc_to_anc
            .clone()
            .into_iter()
            .rev()
            .map(|(cs_id, _)| cs_id)
            .collect::<Vec<_>>();
        let manager = repo.repo_derived_data().manager();

        // Pre-derive HgChangesets for the old HgManifest-based augmented path.
        manager
            .derive_exactly_batch::<MappedHgChangesetId>(ctx, csids.clone(), None)
            .await?;

        // RootAclManifestId is a batch dependency of RootHgAugmentedManifestId
        manager
            .derive_exactly_batch::<RootAclManifestId>(ctx, csids.clone(), None)
            .await?;

        manager
            .derive_exactly_batch::<RootHgAugmentedManifestId>(ctx, csids.clone(), None)
            .await?;

        // Verify HgChangesets match the expected values and that manifests
        // are readable through the reconstruction layer.
        let hg_cs_derived = manager
            .fetch_derived_batch::<MappedHgChangesetId>(ctx, csids, None)
            .await?;
        for (cs_id, expected_hg_cs_id) in commits_desc_to_anc.into_iter().rev() {
            let hg_cs = hg_cs_derived
                .get(&cs_id)
                .unwrap_or_else(|| panic!("HgChangeset not derived for {cs_id}"));
            assert_eq!(
                hg_cs.hg_changeset_id(),
                expected_hg_cs_id,
                "HgChangeset mismatch for {cs_id}",
            );

            // Load the manifest — goes through reconstruction when blobs
            // were skipped.
            let hg_changeset =
                Loadable::load(&hg_cs.hg_changeset_id(), ctx, repo.repo_blobstore()).await?;
            let _manifest =
                Loadable::load(&hg_changeset.manifestid(), ctx, repo.repo_blobstore()).await?;
        }

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_batch_derive(fb: FacebookInit) -> Result<()> {
        verify_repo(fb, || Linear::get_repo::<TestRepo>(fb)).await?;
        verify_repo(fb, || BranchEven::get_repo::<TestRepo>(fb)).await?;
        verify_repo(fb, || BranchUneven::get_repo::<TestRepo>(fb)).await?;
        verify_repo(fb, || BranchWide::get_repo::<TestRepo>(fb)).await?;
        verify_repo(fb, || ManyDiamonds::get_repo::<TestRepo>(fb)).await?;
        verify_repo(fb, || ManyFilesDirs::get_repo::<TestRepo>(fb)).await?;
        verify_repo(fb, || MergeEven::get_repo::<TestRepo>(fb)).await?;
        verify_repo(fb, || MergeUneven::get_repo::<TestRepo>(fb)).await?;
        verify_repo(fb, || UnsharedMergeEven::get_repo::<TestRepo>(fb)).await?;
        verify_repo(fb, || UnsharedMergeUneven::get_repo::<TestRepo>(fb)).await?;
        // Create a repo with a few empty commits in a row
        verify_repo(fb, || async {
            let repo: TestRepo = test_repo_factory::build_empty(fb).await.unwrap();
            let ctx = CoreContext::test_mock(fb);
            let root_empty = CreateCommitContext::new_root(&ctx, &repo)
                .commit()
                .await
                .unwrap();
            let first_empty = CreateCommitContext::new(&ctx, &repo, vec![root_empty])
                .commit()
                .await
                .unwrap();
            let second_empty = CreateCommitContext::new(&ctx, &repo, vec![first_empty])
                .commit()
                .await
                .unwrap();
            let first_non_empty = CreateCommitContext::new(&ctx, &repo, vec![second_empty])
                .add_file("file", "a")
                .commit()
                .await
                .unwrap();
            let third_empty = CreateCommitContext::new(&ctx, &repo, vec![first_non_empty])
                .delete_file("file")
                .commit()
                .await
                .unwrap();
            let fourth_empty = CreateCommitContext::new(&ctx, &repo, vec![third_empty])
                .commit()
                .await
                .unwrap();
            let fifth_empty = CreateCommitContext::new(&ctx, &repo, vec![fourth_empty])
                .commit()
                .await
                .unwrap();

            tests_utils::bookmark(&ctx, &repo, "master")
                .set_to(fifth_empty)
                .await
                .unwrap();
            repo
        })
        .await?;

        verify_repo(fb, || async {
            let repo: TestRepo = test_repo_factory::build_empty(fb).await.unwrap();
            let ctx = CoreContext::test_mock(fb);
            let root = CreateCommitContext::new_root(&ctx, &repo)
                .add_file("dir/subdir/to_replace", "one")
                .add_file("dir/subdir/file", "content")
                .add_file("somefile", "somecontent")
                .commit()
                .await
                .unwrap();
            let modify_unrelated = CreateCommitContext::new(&ctx, &repo, vec![root])
                .add_file("dir/subdir/file", "content2")
                .delete_file("somefile")
                .commit()
                .await
                .unwrap();
            let replace_file_with_dir =
                CreateCommitContext::new(&ctx, &repo, vec![modify_unrelated])
                    .delete_file("dir/subdir/to_replace")
                    .add_file("dir/subdir/to_replace/file", "newcontent")
                    .commit()
                    .await
                    .unwrap();

            tests_utils::bookmark(&ctx, &repo, "master")
                .set_to(replace_file_with_dir)
                .await
                .unwrap();
            repo
        })
        .await?;

        // Weird case - let's delete a file that was already replaced with a directory
        verify_repo(fb, || async {
            let repo: TestRepo = test_repo_factory::build_empty(fb).await.unwrap();
            let ctx = CoreContext::test_mock(fb);
            let root = CreateCommitContext::new_root(&ctx, &repo)
                .add_file("dir/subdir/to_replace", "one")
                .commit()
                .await
                .unwrap();
            let replace_file_with_dir = CreateCommitContext::new(&ctx, &repo, vec![root])
                .delete_file("dir/subdir/to_replace")
                .add_file("dir/subdir/to_replace/file", "newcontent")
                .commit()
                .await
                .unwrap();
            let noop_delete = CreateCommitContext::new(&ctx, &repo, vec![replace_file_with_dir])
                .delete_file("dir/subdir/to_replace")
                .commit()
                .await
                .unwrap();
            let second_noop_delete = CreateCommitContext::new(&ctx, &repo, vec![noop_delete])
                .delete_file("dir/subdir/to_replace")
                .commit()
                .await
                .unwrap();

            tests_utils::bookmark(&ctx, &repo, "master")
                .set_to(second_noop_delete)
                .await
                .unwrap();
            repo
        })
        .await?;

        // Add renames
        verify_repo(fb, || async {
            let repo: TestRepo = test_repo_factory::build_empty(fb).await.unwrap();
            let ctx = CoreContext::test_mock(fb);
            let root = CreateCommitContext::new_root(&ctx, &repo)
                .add_file("dir", "one")
                .commit()
                .await
                .unwrap();
            let renamed = CreateCommitContext::new(&ctx, &repo, vec![root])
                .add_file_with_copy_info("copied_dir", "one", (root, "dir"))
                .commit()
                .await
                .unwrap();
            let after_rename = CreateCommitContext::new(&ctx, &repo, vec![renamed])
                .add_file("new_file", "file")
                .commit()
                .await
                .unwrap();

            tests_utils::bookmark(&ctx, &repo, "master")
                .set_to(after_rename)
                .await
                .unwrap();
            repo
        })
        .await?;

        Ok(())
    }
}
