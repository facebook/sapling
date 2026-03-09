/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

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
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathOrPrefix;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgChangesetId;
use mercurial_types::HgManifestId;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableUntopologicallyVariant;
use mononoke_types::RepoPath;
use restricted_paths::ArcRestrictedPaths;
use restricted_paths::ManifestType;
use restricted_paths::RestrictedPathManifestIdEntry;
use stats::prelude::*;
use tracing::debug;
use tracing::warn;

define_stats! {
    prefix = "mononoke.derived_data.hgchangesets";
    new_parallel: timeseries(Rate, Sum),
}

use derived_data_service_if as thrift;

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
                    .with_context(|| {
                        format!("failed deriving stack of {:?} to {:?}", first, last,)
                    })?;

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

/// Whether to skip writing HG manifest blobs during augmented manifest derivation.
/// When enabled, HG manifest blobs are dropped from the write cache before flush,
/// relying on reconstruction from augmented manifests for subsequent reads.
fn should_skip_hgmanifest_writes(repo_name: &str) -> Result<bool> {
    justknobs::eval("scm/mononoke:hgmanifest_skip_writes", None, Some(repo_name))
}

async fn get_subtree_change_sources(
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
    restricted_paths: ArcRestrictedPaths,
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
    )?;

    if !restricted_paths_enabled {
        return Ok(());
    }

    // Get the configured restricted paths
    let restricted_dirs: Vec<PathOrPrefix> = restricted_paths
        .config()
        .path_acls
        .keys()
        .map(|non_root_mpath| PathOrPrefix::Path(non_root_mpath.clone().into()))
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
    format!("{}{}{}", root_prefix, key_prefix, cs_id)
}

/// Gets the HgManifestId for a commit, optionally deriving the HgChangeset inline.
///
/// Checks for the HgChangesetId in the following order:
/// 1. The `known_hg_cs` map (batch-internal cache of already-derived changesets)
/// 2. The persisted mapping (SQL via fetch_derived)
/// 3. Derives inline if not found anywhere
///
/// Returns the manifest ID and optionally the inline-derived MappedHgChangesetId
/// (so the caller can track it for subsequent commits in the same batch).
async fn get_manifest_id(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
    known_hg_cs: Option<&HashMap<ChangesetId, MappedHgChangesetId>>,
) -> Result<(HgManifestId, Option<MappedHgChangesetId>)> {
    let csid = bonsai.get_changeset_id();
    let blobstore = derivation_ctx.blobstore();

    // Check batch-internal cache first.
    if let Some(hg_cs) = known_hg_cs.and_then(|m| m.get(&csid)) {
        let manifestid = hg_cs
            .hg_changeset_id()
            .load(ctx, blobstore)
            .await?
            .manifestid();
        return Ok((manifestid, None));
    }

    // Check persisted mapping (SQL).
    if let Some(hg_cs) = derivation_ctx
        .fetch_derived::<MappedHgChangesetId>(ctx, csid)
        .await?
    {
        let manifestid = hg_cs
            .hg_changeset_id()
            .load(ctx, blobstore)
            .await?
            .manifestid();
        return Ok((manifestid, None));
    }

    // Not found — derive HgChangeset inline.
    let parents: Vec<_> = bonsai.parents().collect();
    let mut hg_parents = Vec::with_capacity(parents.len());
    for parent in &parents {
        if let Some(hg_cs) = known_hg_cs.and_then(|m| m.get(parent)) {
            hg_parents.push(hg_cs.clone());
        } else if let Some(hg_cs) = derivation_ctx
            .fetch_derived::<MappedHgChangesetId>(ctx, *parent)
            .await?
        {
            hg_parents.push(hg_cs);
        }
    }
    anyhow::ensure!(
        hg_parents.len() == parents.len(),
        "Tried to derive MappedHgChangesetId for {}, but {} of {} parents were not ready",
        csid,
        parents.len() - hg_parents.len(),
        parents.len()
    );

    let empty = HashMap::new();
    let mapping = known_hg_cs.unwrap_or(&empty);
    let subtree_change_sources =
        get_subtree_change_sources(ctx, derivation_ctx, bonsai, mapping).await?;
    let derivation_opts = get_hg_changeset_derivation_options(derivation_ctx);
    let (derived, manifestid) = crate::derive_hg_changeset::derive_from_parents(
        ctx,
        derivation_ctx.blobstore(),
        bonsai.clone(),
        hg_parents,
        subtree_change_sources,
        &derivation_opts,
        derivation_ctx.restricted_paths(),
    )
    .await?;
    Ok((manifestid, Some(derived)))
}

/// Store a MappedHgChangesetId mapping to SQL (BonsaiHgMapping).
///
/// This is a direct SQL write (not through the blobstore write cache), so it's
/// immediately persistent and visible to subsequent batch segments and other
/// processes.
async fn store_hg_cs_mapping(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    cs_id: ChangesetId,
    hg_cs: &MappedHgChangesetId,
) -> Result<()> {
    derivation_ctx
        .bonsai_hg_mapping()?
        .add(
            ctx,
            BonsaiHgMappingEntry {
                hg_cs_id: hg_cs.hg_changeset_id(),
                bcs_id: cs_id,
            },
        )
        .await?;
    Ok(())
}

#[async_trait]
impl BonsaiDerivable for RootHgAugmentedManifestId {
    const VARIANT: DerivableType = DerivableType::HgAugmentedManifests;

    type Dependencies = dependencies![];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
        _known: Option<&HashMap<ChangesetId, Self>>,
    ) -> Result<Self> {
        let blobstore = derivation_ctx.blobstore();

        let content_ids = bonsai
            .file_changes()
            .filter_map(|(_path, change)| change.simplify().map(|change| change.content_id()))
            .collect::<HashSet<_>>();
        let content_metadata_fut = prefetch_content_metadata(ctx, blobstore, content_ids);

        let csid = bonsai.get_changeset_id();

        let ((hg_manifest_id, derived_hg_cs), content_metadata) = future::try_join(
            get_manifest_id(ctx, derivation_ctx, &bonsai, None),
            content_metadata_fut,
        )
        .await?;

        // Derive augmented manifests before flushing. The HgManifest blobs
        // are still in the MemWritesBlobstore write cache, so augmented
        // manifest derivation can read them directly via .load().
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
            )
            .await?;

        // If we inline-derived a HgChangeset, flush blobs to persistent
        // storage. When skipping HgManifest writes, remove those blobs
        // from the write cache first — augmented manifests have already
        // been derived above and no longer need them.
        if let Some(hg_cs) = derived_hg_cs {
            if should_skip_hgmanifest_writes(derivation_ctx.repo_name())? {
                derivation_ctx.remove_write_cache_by_prefix("hgmanifest.");
            }
            derivation_ctx.flush(ctx).await?;
            store_hg_cs_mapping(ctx, derivation_ctx, csid, &hg_cs).await?;
        }

        Ok(Self(root_hg_aug_mfid))
    }

    async fn derive_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
    ) -> Result<HashMap<ChangesetId, Self>> {
        let blobstore = derivation_ctx.blobstore();
        let mut res: HashMap<ChangesetId, Self> = HashMap::new();
        let mut hg_cs_map: HashMap<ChangesetId, MappedHgChangesetId> = HashMap::new();

        for bonsai in &bonsais {
            let csid = bonsai.get_changeset_id();

            let content_ids = bonsai
                .file_changes()
                .filter_map(|(_path, change)| change.simplify().map(|change| change.content_id()))
                .collect::<HashSet<_>>();
            let content_metadata_fut = prefetch_content_metadata(ctx, blobstore, content_ids);

            let ((hg_manifest_id, derived_hg_cs), content_metadata) = future::try_join(
                get_manifest_id(ctx, derivation_ctx, bonsai, Some(&hg_cs_map)),
                content_metadata_fut,
            )
            .await?;

            if let Some(hg_cs) = derived_hg_cs {
                hg_cs_map.insert(csid, hg_cs);
            }

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
            )
            .await?;

            res.insert(csid, Self(root));
        }

        if should_skip_hgmanifest_writes(derivation_ctx.repo_name())? {
            // Remove HgManifest blobs from the write cache before flushing.
            // The augmented manifest is a superset of HgManifest data, so these
            // blobs can be reconstructed from it later if needed (see the
            // reconstruction layer in fetch_manifest_envelope_opt). Skipping
            // these writes reduces PUTs by ~33% for augmented manifest derivation.
            derivation_ctx.remove_write_cache_by_prefix("hgmanifest.");
        }

        // Flush all blob writes BEFORE storing SQL mappings.
        // This ensures that HgChangeset blobs are in persistent storage
        // before their SQL mappings become visible to other processes.
        derivation_ctx.flush(ctx).await?;

        // Now safe to store — blobs are in persistent storage.
        let entries: Vec<BonsaiHgMappingEntry> = hg_cs_map
            .iter()
            .map(|(csid, hg_cs)| BonsaiHgMappingEntry {
                hg_cs_id: hg_cs.hg_changeset_id(),
                bcs_id: *csid,
            })
            .collect();
        derivation_ctx
            .bonsai_hg_mapping()?
            .bulk_add(ctx, &entries)
            .await?;

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
    type PredecessorDependencies = dependencies![MappedHgChangesetId];

    async fn unsafe_derive_untopologically(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
    ) -> Result<Self> {
        let hg_changeset_id = derivation_ctx
            .fetch_dependency::<MappedHgChangesetId>(ctx, bonsai.get_changeset_id())
            .await?
            .hg_changeset_id();
        let hg_manifest_id = hg_changeset_id
            .load(ctx, derivation_ctx.blobstore())
            .await?
            .manifestid();
        let root = crate::derive_hg_augmented_manifest::derive_from_full_hg_manifest(
            ctx.clone(),
            Arc::clone(derivation_ctx.blobstore()),
            hg_manifest_id,
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
    use futures::TryStreamExt;
    use justknobs::test_helpers::JustKnobsInMemory;
    use justknobs::test_helpers::KnobVal;
    use justknobs::test_helpers::override_just_knobs;
    use mononoke_macros::mononoke;
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

    /// Like `verify_repo`, but derives `RootHgAugmentedManifestId` (which
    /// internally derives HgChangesets).  When the skip-writes knob is
    /// active, this exercises the write-skipping path and verifies that
    /// HgManifest blobs can still be read through the reconstruction layer.
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

        manager
            .derive_exactly_batch::<RootHgAugmentedManifestId>(ctx, csids.clone(), None)
            .await?;

        // Verify HgChangesets (inline-derived by augmented manifest derivation)
        // match the expected values and that manifests are readable through
        // the reconstruction layer (HgManifest blobs were skipped).
        let hg_cs_derived = manager
            .fetch_derived_batch::<MappedHgChangesetId>(ctx, csids, None)
            .await?;
        for (cs_id, expected_hg_cs_id) in commits_desc_to_anc.into_iter().rev() {
            let hg_cs = hg_cs_derived
                .get(&cs_id)
                .unwrap_or_else(|| panic!("HgChangeset not derived for {}", cs_id));
            assert_eq!(
                hg_cs.hg_changeset_id(),
                expected_hg_cs_id,
                "HgChangeset mismatch for {}",
                cs_id,
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

    #[mononoke::fbinit_test]
    async fn test_batch_derive_skip_hgmanifest_writes(fb: FacebookInit) -> Result<()> {
        override_just_knobs(JustKnobsInMemory::new(HashMap::from([(
            "scm/mononoke:hgmanifest_skip_writes".to_string(),
            KnobVal::Bool(true),
        )])));
        verify_repo_aug(fb, || Linear::get_repo::<TestRepo>(fb)).await?;
        verify_repo_aug(fb, || BranchEven::get_repo::<TestRepo>(fb)).await?;
        verify_repo_aug(fb, || BranchUneven::get_repo::<TestRepo>(fb)).await?;
        verify_repo_aug(fb, || BranchWide::get_repo::<TestRepo>(fb)).await?;
        verify_repo_aug(fb, || ManyDiamonds::get_repo::<TestRepo>(fb)).await?;
        verify_repo_aug(fb, || ManyFilesDirs::get_repo::<TestRepo>(fb)).await?;
        verify_repo_aug(fb, || MergeEven::get_repo::<TestRepo>(fb)).await?;
        verify_repo_aug(fb, || MergeUneven::get_repo::<TestRepo>(fb)).await?;
        verify_repo_aug(fb, || UnsharedMergeEven::get_repo::<TestRepo>(fb)).await?;
        verify_repo_aug(fb, || UnsharedMergeUneven::get_repo::<TestRepo>(fb)).await?;
        Ok(())
    }
}
