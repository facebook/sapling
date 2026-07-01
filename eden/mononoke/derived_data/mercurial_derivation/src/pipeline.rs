/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context as _;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use blobrepo_common::changed_files::compute_changed_files;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMappingEntry;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use derived_data_manager::DerivationStagePayload;
use derived_data_manager::PipelineDerivable;
use derived_data_manager::StageId;
use fbthrift::compact_protocol;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathOrPrefix;
use manifest::Traced;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mercurial_types::subtree::HgSubtreeChanges;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use mononoke_types::PipelineDerivableVariant;
use mononoke_types::ThriftConvert;
use mononoke_types::path::MPath;

use crate::derive_hg_changeset::generate_hg_changeset;
use crate::derive_hg_changeset::get_manifest_entry_from_bonsai;
use crate::derive_hg_manifest::ParentIndex;
use crate::mapping::HgChangesetDeriveOptions;
use crate::mapping::MappedHgChangesetId;
use crate::mapping::get_subtree_change_sources;

/// Output of a single pipeline stage for `MappedHgChangesetId`.
///
/// `entry` carries the manifest entry at the stage's configured path,
/// preserving its `Traced<ParentIndex, _>` lineage from the producing
/// stage so downstream stages can plug it back into `derive_manifest`
/// via `known_entries` without rewrapping.
///
/// `hg_cs_id` is set only by the finalize stage (manifest stages leave it
/// `None`): it carries each parent's hg changeset id through `parents` so the
/// finalize chain can hash the next commit's changeset without a
/// `bonsai_hg_mapping` lookup for in-batch parents.
#[derive(Clone, Debug, PartialEq)]
pub struct HgStageOutput {
    pub entry: Option<
        Entry<Traced<ParentIndex, HgManifestId>, Traced<ParentIndex, (FileType, HgFileNodeId)>>,
    >,
    pub hg_cs_id: Option<HgChangesetId>,
}

/// Stage-output key format version; bump on any format change so older binaries read a disjoint keyspace (re-derive) instead of misreading. v2: hg_cs_id moved to the finalize stage.
const STAGE_OUTPUT_FORMAT_VERSION: u32 = 2;

fn stage_blobstore_key(stage_path: &MPath, key_prefix: &str, cs_id: ChangesetId) -> String {
    format!(
        "derived_hg_manifest_stage.v{STAGE_OUTPUT_FORMAT_VERSION}.{}.{key_prefix}{cs_id}",
        stage_path.get_path_hash().to_hex(),
    )
}

/// Blobstore key for the finalize stage output (transitional, used while the
/// prod-mapping knob is off). Distinct from the manifest stage keys, which are
/// keyed by stage path hash.
fn finalize_blobstore_key(key_prefix: &str, cs_id: ChangesetId) -> String {
    format!("derived_hg_manifest_stage.v{STAGE_OUTPUT_FORMAT_VERSION}.finalize.{key_prefix}{cs_id}")
}

fn untraced_manifest_id(
    entry: &Entry<Traced<ParentIndex, HgManifestId>, Traced<ParentIndex, (FileType, HgFileNodeId)>>,
) -> Option<HgManifestId> {
    match entry {
        Entry::Tree(t) => Some(*t.untraced()),
        Entry::Leaf(_) => None,
    }
}

/// Strip the `Traced` wrappers from a stage-output entry. Used at stage
/// boundaries before handing entries to `get_manifest_entry_from_bonsai`,
/// which re-assigns `ParentIndex` by bonsai-parent position.
pub(crate) fn untrace_entry(
    entry: Entry<Traced<ParentIndex, HgManifestId>, Traced<ParentIndex, (FileType, HgFileNodeId)>>,
) -> Entry<HgManifestId, (FileType, HgFileNodeId)> {
    match entry {
        Entry::Tree(t) => Entry::Tree(t.into_untraced()),
        Entry::Leaf(l) => Entry::Leaf(l.into_untraced()),
    }
}

/// Wrap an untraced Entry with `Traced::generate` on both arms. Used when
/// reconstructing a stage output from a non-pipelined derivation: lineage is
/// not recoverable from a SQL-only or post-hoc load, so every leaf and tree
/// is tagged as "generated" (no specific parent index).
fn wrap_entry_traced(
    entry: Entry<HgManifestId, (FileType, HgFileNodeId)>,
) -> Entry<Traced<ParentIndex, HgManifestId>, Traced<ParentIndex, (FileType, HgFileNodeId)>> {
    match entry {
        Entry::Tree(m) => Entry::Tree(Traced::generate(m)),
        Entry::Leaf(l) => Entry::Leaf(Traced::generate(l)),
    }
}

/// Resolve cross-stage copy sources for a single bonsai at `cur_stage_path`.
///
/// A cross-stage copy is a file change under `cur_stage_path` whose `copy_from`
/// references a real parent (p1/p2) at a source path outside `cur_stage_path`.
/// The source filenode lives in that parent's full root hg manifest, which the
/// per-stage output doesn't carry, so we resolve the parent's terminal (root)
/// stage output and look the source filenode up in its root manifest. The
/// terminal stage output is resolved the same two-tier way the manager resolves
/// the `parents` map: read the pipeline terminal stage output, falling back to
/// extracting it from the canonical derived value for parents that predate the
/// pipeline. Returns a map keyed by destination path.
async fn resolve_cross_stage_copy_sources(
    ctx: &CoreContext,
    derivation: &DerivationContext,
    blobstore: &std::sync::Arc<dyn blobstore::KeyedBlobstore>,
    bonsai: &BonsaiChangeset,
    cur_stage_path: &MPath,
) -> Result<HashMap<mononoke_types::NonRootMPath, (mononoke_types::NonRootMPath, HgFileNodeId)>> {
    use mononoke_types::FileChange;

    let parents: Vec<ChangesetId> = bonsai.parents().collect();
    let p1 = parents.first().copied();
    let p2 = parents.get(1).copied();

    // Collect cross-stage copies as (dest, copy_path, parent_csid).
    let mut pending: Vec<(
        mononoke_types::NonRootMPath,
        mononoke_types::NonRootMPath,
        ChangesetId,
    )> = Vec::new();
    for (path, fc) in bonsai.file_changes() {
        // Keep dests at or under the stage; skip dests strictly outside it.
        if !cur_stage_path.is_prefix_of(path) {
            continue;
        }
        let FileChange::Change(tc) = fc else {
            continue;
        };
        let Some((copy_path, bcsid)) = tc.copy_from() else {
            continue;
        };
        let is_real_parent = Some(*bcsid) == p1 || Some(*bcsid) == p2;
        if !is_real_parent {
            continue;
        }
        // Only sources strictly outside the stage need the parent terminal root;
        // sources at or under the stage are resolved from the parent stage output.
        if cur_stage_path.is_prefix_of(copy_path) {
            continue;
        }
        pending.push((path.clone(), copy_path.clone(), *bcsid));
    }

    if pending.is_empty() {
        return Ok(HashMap::new());
    }

    let mut needed: Vec<ChangesetId> = pending.iter().map(|(_, _, csid)| *csid).collect();
    needed.sort();
    needed.dedup();

    // Resolve each source parent's terminal (root) stage output, mirroring the
    // manager's transitionary parent resolution: pipeline terminal stage output
    // first, falling back to the canonical derived value for parents that have
    // no pipeline terminal output yet. Pre-flip the canonical fallback leans on
    // bonsai_hg_mapping exactly as the manager does; we never read it directly.
    let mut terminal_outputs = MappedHgChangesetId::fetch_stage_outputs(
        ctx,
        derivation,
        &StageId::Manifest(MPath::ROOT),
        needed.clone(),
    )
    .await?;
    let missing: Vec<ChangesetId> = needed
        .iter()
        .copied()
        .filter(|csid| !terminal_outputs.contains_key(csid))
        .collect();
    let fetched: Vec<(ChangesetId, HgStageOutput)> = stream::iter(missing)
        .map(|parent_csid| async move {
            let derived = derivation
                .fetch_dependency::<MappedHgChangesetId>(ctx, parent_csid)
                .await
                .with_context(|| {
                    format!(
                        "resolving cross-stage copy source parent {parent_csid}: no pipeline terminal stage output and canonical hg changeset not derived"
                    )
                })?;
            let output = MappedHgChangesetId::extract_stage_output_from_derived(
                ctx,
                derivation,
                &derived,
                &StageId::Manifest(MPath::ROOT),
            )
            .await?;
            Ok::<_, Error>((parent_csid, output))
        })
        .buffer_unordered(10)
        .try_collect()
        .await?;
    terminal_outputs.extend(fetched);

    // Group cross-stage copies by source parent so each parent needs only a
    // single manifest traversal.
    let mut by_parent: HashMap<
        ChangesetId,
        Vec<(mononoke_types::NonRootMPath, mononoke_types::NonRootMPath)>,
    > = HashMap::new();
    for (dest, copy_path, parent_csid) in pending {
        by_parent
            .entry(parent_csid)
            .or_default()
            .push((dest, copy_path));
    }

    // Resolve each parent concurrently: take its root hg manifest id from the
    // terminal stage output, then do a single `find_entries` for all of that
    // parent's copy sources. Mirrors canonical `resolve_paths`: copies whose
    // source is absent in the parent, or is not a file, are silently dropped.
    let resolved: HashMap<
        mononoke_types::NonRootMPath,
        (mononoke_types::NonRootMPath, HgFileNodeId),
    > = stream::iter(by_parent)
        .map(|(parent_csid, copies)| {
            let terminal_outputs = &terminal_outputs;
            let ctx = ctx.clone();
            let blobstore = blobstore.clone();
            async move {
                let output = terminal_outputs.get(&parent_csid).ok_or_else(|| {
                    anyhow!(
                        "no terminal stage output resolved for cross-stage copy source parent {parent_csid}"
                    )
                })?;
                let root_manifest = output
                    .entry
                    .as_ref()
                    .and_then(untraced_manifest_id)
                    .ok_or_else(|| {
                        anyhow!(
                            "terminal stage output for cross-stage copy source parent {parent_csid} has no root manifest entry"
                        )
                    })?;

                let found: HashMap<MPath, (FileType, HgFileNodeId)> = root_manifest
                    .find_entries(
                        ctx,
                        blobstore,
                        copies
                            .iter()
                            .map(|(_, copy_path)| PathOrPrefix::Path(MPath::from(copy_path.clone()))),
                    )
                    .try_filter_map(|(path, entry)| async move {
                        Ok(entry.into_leaf().map(|leaf| (path, leaf)))
                    })
                    .try_collect()
                    .await?;

                let resolved_for_parent: Vec<(
                    mononoke_types::NonRootMPath,
                    (mononoke_types::NonRootMPath, HgFileNodeId),
                )> = copies
                    .into_iter()
                    .filter_map(|(dest, copy_path)| {
                        found
                            .get(&MPath::from(copy_path.clone()))
                            .map(|(_file_type, filenode)| (dest, (copy_path, *filenode)))
                    })
                    .collect();
                Ok::<_, Error>(resolved_for_parent)
            }
        })
        .buffer_unordered(10)
        .try_concat()
        .await?
        .into_iter()
        .collect();

    Ok(resolved)
}

#[async_trait]
impl PipelineDerivable for MappedHgChangesetId {
    const PIPELINE_DERIVABLE_VARIANT: PipelineDerivableVariant =
        PipelineDerivableVariant::HgChangesets;

    const HAS_FINALIZE: bool = true;

    type StageOutput = HgStageOutput;

    async fn derive_stage_batch(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
        payload: &DerivationStagePayload,
        parents: HashMap<ChangesetId, Self::StageOutput>,
        dependency_outputs: HashMap<ChangesetId, HashMap<MPath, Self::StageOutput>>,
    ) -> Result<HashMap<ChangesetId, Self::StageOutput>> {
        let blobstore = derivation.blobstore();

        let payload = match payload {
            DerivationStagePayload::Manifest(payload) => payload,
            DerivationStagePayload::Finalize => {
                return finalize_stage_batch(ctx, derivation, bonsais, parents, dependency_outputs)
                    .await;
            }
        };
        let cur_stage_path = &payload.path;

        let mut results: HashMap<ChangesetId, HgStageOutput> = HashMap::new();

        for bonsai in bonsais {
            let cs_id = bonsai.get_changeset_id();

            // Build parent stage outputs in bonsai-parent order, preferring
            // results from this batch over the externally supplied `parents`.
            let parent_outputs: Vec<HgStageOutput> = bonsai
                .parents()
                .map(|parent_csid| {
                    results
                        .get(&parent_csid)
                        .or_else(|| parents.get(&parent_csid))
                        .cloned()
                        .ok_or_else(|| {
                            anyhow!("missing stage output for parent {parent_csid} of {cs_id}")
                        })
                })
                .collect::<Result<Vec<_>>>()?;

            // Parent entries at stage_path, in bonsai-parent order. Positional
            // slots are preserved so `get_manifest_entry_from_bonsai` can align
            // `parent_entries[i]` with `bcs.parents().nth(i)` for copy-from
            // filenode lookup. `None` means the parent has nothing at
            // stage_path. The Traced wrappers from the producing stage are
            // stripped; the callee re-tags by bonsai-parent position.
            let parent_entries: Vec<Option<Entry<HgManifestId, (FileType, HgFileNodeId)>>> =
                parent_outputs
                    .iter()
                    .map(|out| out.entry.clone().map(untrace_entry))
                    .collect();

            // Build `known_entries` from dependency stage outputs. The manager
            // keys dependency_outputs by absolute dep path, so we can copy
            // entries straight in.
            let known_entries: HashMap<
                MPath,
                Option<
                    Entry<
                        Traced<ParentIndex, HgManifestId>,
                        Traced<ParentIndex, (FileType, HgFileNodeId)>,
                    >,
                >,
            > = dependency_outputs
                .get(&cs_id)
                .map(|deps| {
                    deps.iter()
                        .map(|(dep_path, dep_output)| (dep_path.clone(), dep_output.entry.clone()))
                        .collect()
                })
                .unwrap_or_default();

            // Subtree changes are an orthogonal Mercurial feature, but each
            // stage must consider them because file changes under `stage_path`
            // may reference subtree-copy sources. The `derivation_pipeline_tailer`
            // splits batches so any commit with manifest-altering subtree changes
            // arrives in a single-commit batch, after its subtree-copy sources
            // have been fully derived in an earlier batch. So we never have
            // in-batch sources to short-circuit — `bonsai_hg_mapping` always
            // has every source we need.
            let subtree_change_sources =
                get_subtree_change_sources(ctx, derivation, &bonsai, &HashMap::new()).await?;
            let subtree_changes = HgSubtreeChanges::from_bonsai_subtree_changes(
                bonsai.subtree_changes(),
                subtree_change_sources,
            )?;

            // Resolve cross-stage copy sources: a file change under
            // `cur_stage_path` whose `copy_from` source is a real parent
            // (p1/p2) but lies outside `cur_stage_path`. The source filenode
            // lives in the parent's full root manifest, which the stage output
            // doesn't carry, so we load the parent's root hg envelope and look
            // it up there. Keyed by destination path.
            let cross_stage_copy_sources = resolve_cross_stage_copy_sources(
                ctx,
                derivation,
                blobstore,
                &bonsai,
                cur_stage_path,
            )
            .await?;

            let entry = get_manifest_entry_from_bonsai(
                ctx.clone(),
                blobstore.clone(),
                derivation.restricted_paths(),
                bonsai.clone(),
                parent_entries,
                subtree_changes.as_ref(),
                cur_stage_path.clone(),
                known_entries,
                cross_stage_copy_sources,
            )
            .await?;

            // Manifest stages never finalize; the hg changeset is produced by
            // the separate finalize stage.
            results.insert(
                cs_id,
                HgStageOutput {
                    entry,
                    hg_cs_id: None,
                },
            );
        }

        Ok(results)
    }

    async fn extract_stage_output_from_derived(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        derived: &MappedHgChangesetId,
        stage: &StageId,
    ) -> Result<Self::StageOutput> {
        let hg_cs_id = derived.hg_changeset_id();
        let envelope = hg_cs_id.load(ctx, derivation.blobstore()).await?;
        let root_mfid = envelope.manifestid();
        // A manifest stage is anchored at its configured path; the finalize
        // stage is anchored at the root manifest and additionally carries the
        // hg changeset id.
        let stage_path = match stage {
            StageId::Manifest(stage_path) => stage_path.clone(),
            StageId::Finalize => MPath::ROOT,
        };
        let raw_entry = root_mfid
            .find_entry(ctx.clone(), derivation.blobstore().clone(), stage_path)
            .await?;
        // Lineage isn't recoverable from non-pipelined derivation. Wrap with
        // Traced::generate, matching the convention used by derive_hg_manifest
        // for entries that didn't come from a specific input parent.
        // Functionally safe because known_entries short-circuit
        // MergeResult::Reuse without consulting Traced state.
        let entry = raw_entry.map(wrap_entry_traced);
        let hg_cs_id = matches!(stage, StageId::Finalize).then_some(hg_cs_id);
        Ok(HgStageOutput { entry, hg_cs_id })
    }

    async fn store_stage_outputs(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        stage: &StageId,
        outputs: HashMap<ChangesetId, Self::StageOutput>,
    ) -> Result<()> {
        match stage {
            StageId::Manifest(stage_path) => {
                // Manifest stages are never the canonical mapping; always write
                // the thrift-serialized stage output to the blobstore.
                let key_prefix = derivation.mapping_key_prefix::<MappedHgChangesetId>();
                stream::iter(outputs.into_iter().map(|(cs_id, output)| async move {
                    let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                    let thrift_output = stage_output_to_thrift(&output)?;
                    let bytes = compact_protocol::serialize(&thrift_output);
                    derivation
                        .blobstore()
                        .put(ctx, key, BlobstoreBytes::from_bytes(bytes))
                        .await
                }))
                .buffered(100)
                .try_collect::<Vec<_>>()
                .await?;
                Ok(())
            }
            StageId::Finalize => {
                let use_normal_mapping = justknobs::eval(
                    "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
                    None,
                    Some("hg_changesets"),
                );

                if use_normal_mapping {
                    // The finalize stage is the canonical terminal: write the
                    // bonsai_hg_mapping SQL row and skip the blobstore.
                    let entries: Vec<BonsaiHgMappingEntry> = outputs
                        .iter()
                        .map(|(bcs_id, output)| {
                            let hg_cs_id = output.hg_cs_id.ok_or_else(|| {
                                anyhow!(
                                    "finalize stage output for {bcs_id} missing hg_cs_id (cannot write bonsai_hg_mapping)",
                                )
                            })?;
                            Ok(BonsaiHgMappingEntry {
                                hg_cs_id,
                                bcs_id: *bcs_id,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    derivation
                        .bonsai_hg_mapping()?
                        .bulk_add(ctx, &entries)
                        .await?;
                    return Ok(());
                }

                // Prod-mapping off (transitional): write the thrift-serialized
                // terminal stage output (hg_cs_id + root manifest id) to the
                // blobstore under the finalize key.
                let key_prefix = derivation.mapping_key_prefix::<MappedHgChangesetId>();
                stream::iter(outputs.into_iter().map(|(cs_id, output)| async move {
                    let key = finalize_blobstore_key(key_prefix, cs_id);
                    let thrift_output = stage_output_to_thrift(&output)?;
                    let bytes = compact_protocol::serialize(&thrift_output);
                    derivation
                        .blobstore()
                        .put(ctx, key, BlobstoreBytes::from_bytes(bytes))
                        .await
                }))
                .buffered(100)
                .try_collect::<Vec<_>>()
                .await?;
                Ok(())
            }
        }
    }

    async fn fetch_stage_outputs(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        stage: &StageId,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::StageOutput>> {
        match stage {
            StageId::Manifest(stage_path) => {
                // Read the thrift-serialized stage output from the blobstore.
                // Returns None for any cs_id without stored output (skipped).
                let key_prefix = derivation.mapping_key_prefix::<MappedHgChangesetId>();
                let results = stream::iter(cs_ids.into_iter().map(|cs_id| async move {
                    let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                    let Some(blob_data) = derivation.blobstore().get(ctx, &key).await? else {
                        return Ok::<_, Error>(None);
                    };
                    let thrift_output: mercurial_thrift::HgManifestStageOutput =
                        compact_protocol::deserialize(blob_data.into_raw_bytes())?;
                    let output = stage_output_from_thrift(cs_id, thrift_output)?;
                    Ok(Some((cs_id, output)))
                }))
                .buffer_unordered(100)
                .try_filter_map(|opt| async move { Ok(opt) })
                .try_collect::<HashMap<_, _>>()
                .await?;
                Ok(results)
            }
            StageId::Finalize => {
                let use_normal_mapping = justknobs::eval(
                    "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
                    None,
                    Some("hg_changesets"),
                );

                if use_normal_mapping {
                    // Read from bonsai_hg_mapping, then materialize the root
                    // manifest entry for each commit. Wrap with Traced::generate
                    // as the lineage isn't recoverable from a SQL-only derivation.
                    let entries = derivation
                        .bonsai_hg_mapping()?
                        .get(ctx, cs_ids.clone().into())
                        .await?;
                    let hg_by_bcs: HashMap<ChangesetId, HgChangesetId> = entries
                        .into_iter()
                        .map(|e| (e.bcs_id, e.hg_cs_id))
                        .collect();

                    let results =
                        stream::iter(hg_by_bcs.into_iter().map(|(bcs_id, hg_cs_id)| async move {
                            let envelope = hg_cs_id.load(ctx, derivation.blobstore()).await?;
                            let root_mfid = envelope.manifestid();
                            let raw_entry = root_mfid
                                .find_entry(
                                    ctx.clone(),
                                    derivation.blobstore().clone(),
                                    MPath::ROOT,
                                )
                                .await?;
                            let entry = raw_entry.map(wrap_entry_traced);
                            Ok::<_, Error>((
                                bcs_id,
                                HgStageOutput {
                                    entry,
                                    hg_cs_id: Some(hg_cs_id),
                                },
                            ))
                        }))
                        .buffer_unordered(100)
                        .try_collect::<HashMap<_, _>>()
                        .await?;
                    return Ok(results);
                }

                // Prod-mapping off (transitional): read the thrift-serialized
                // terminal stage output from the finalize blob key.
                let key_prefix = derivation.mapping_key_prefix::<MappedHgChangesetId>();
                let results = stream::iter(cs_ids.into_iter().map(|cs_id| async move {
                    let key = finalize_blobstore_key(key_prefix, cs_id);
                    let Some(blob_data) = derivation.blobstore().get(ctx, &key).await? else {
                        return Ok::<_, Error>(None);
                    };
                    let thrift_output: mercurial_thrift::HgManifestStageOutput =
                        compact_protocol::deserialize(blob_data.into_raw_bytes())?;
                    let output = stage_output_from_thrift(cs_id, thrift_output)?;
                    Ok(Some((cs_id, output)))
                }))
                .buffer_unordered(100)
                .try_filter_map(|opt| async move { Ok(opt) })
                .try_collect::<HashMap<_, _>>()
                .await?;
                Ok(results)
            }
        }
    }
}

/// Derive the finalize stage for a batch: assemble each commit's hg changeset
/// from its root manifest (resolved as the finalize stage's dependency) and its
/// parents' hg changeset ids (the finalize chain), and write the hg envelope.
async fn finalize_stage_batch(
    ctx: &CoreContext,
    derivation: &DerivationContext,
    bonsais: Vec<BonsaiChangeset>,
    parents: HashMap<ChangesetId, HgStageOutput>,
    dependency_outputs: HashMap<ChangesetId, HashMap<MPath, HgStageOutput>>,
) -> Result<HashMap<ChangesetId, HgStageOutput>> {
    let blobstore = derivation.blobstore();
    let derivation_opts = HgChangesetDeriveOptions {
        set_committer_field: derivation.config().hg_set_committer_extra,
    };

    // Root manifest id of any commit: from the root manifest stage — its
    // dependency output if the commit is in this batch, otherwise the parent's
    // finalize output (which carries the root manifest entry). All available
    // before the serial hashing pass.
    let manifest_id_of = |id: &ChangesetId| -> Option<HgManifestId> {
        let out = dependency_outputs
            .get(id)
            .and_then(|deps| deps.get(&MPath::ROOT))
            .or_else(|| parents.get(id))?;
        out.entry.as_ref().and_then(untraced_manifest_id)
    };

    // Compute each commit's changed-file list in parallel. It depends only on
    // manifest ids (all known up front), not on the parent hg-changeset chain,
    // so it runs ahead of the serial hashing pass below. Resolve the manifest
    // ids synchronously first (borrowing the dependency/parent maps), then fan
    // out the manifest diffs over owned inputs.
    let file_inputs: Vec<(
        ChangesetId,
        HgManifestId,
        Option<HgManifestId>,
        Option<HgManifestId>,
        bool,
    )> = bonsais
        .iter()
        .map(|bonsai| {
            let cs_id = bonsai.get_changeset_id();
            let manifest_id = manifest_id_of(&cs_id)
                .ok_or_else(|| anyhow!("finalize for {cs_id}: missing root manifest"))?;
            let mut parent_mfs = bonsai.parents().map(|p| manifest_id_of(&p));
            let mf_p1 = parent_mfs.next().flatten();
            let mf_p2 = parent_mfs.next().flatten();
            // Subtree copies make generate_hg_changeset drop the file list, so skip the diff then.
            let has_subtree_copies = bonsai
                .subtree_changes()
                .iter()
                .any(|(_, change)| change.copy_source().is_some());
            Ok((cs_id, manifest_id, mf_p1, mf_p2, has_subtree_copies))
        })
        .collect::<Result<_>>()?;

    let mut files_by_cs: HashMap<ChangesetId, Vec<NonRootMPath>> = stream::iter(file_inputs)
        .map(
            |(cs_id, manifest_id, mf_p1, mf_p2, has_subtree_copies)| async move {
                let files = if has_subtree_copies {
                    Vec::new()
                } else {
                    compute_changed_files(ctx.clone(), blobstore.clone(), manifest_id, mf_p1, mf_p2)
                        .await?
                };
                anyhow::Ok((cs_id, files))
            },
        )
        .buffer_unordered(100)
        .try_collect()
        .await?;

    // Serial hashing pass: each commit's hg changeset id chains off its
    // parents', so this runs in topological order.
    let mut results: HashMap<ChangesetId, HgStageOutput> = HashMap::new();
    for bonsai in bonsais {
        let cs_id = bonsai.get_changeset_id();

        let root_manifest_entry = dependency_outputs
            .get(&cs_id)
            .and_then(|deps| deps.get(&MPath::ROOT))
            .and_then(|out| out.entry.clone())
            .ok_or_else(|| {
                anyhow!("finalize for {cs_id}: missing root manifest dependency output")
            })?;
        let manifest_id = untraced_manifest_id(&root_manifest_entry).ok_or_else(|| {
            anyhow!("finalize for {cs_id}: root manifest dependency is not a tree")
        })?;

        // Parent hg changeset ids come straight from the parents' finalize
        // outputs (preferring in-batch results) — no envelope loads.
        let parent_hg_cs_ids: Vec<HgChangesetId> = bonsai
            .parents()
            .map(|parent_csid| {
                results
                    .get(&parent_csid)
                    .or_else(|| parents.get(&parent_csid))
                    .and_then(|out| out.hg_cs_id)
                    .ok_or_else(|| {
                        anyhow!("finalize parent {parent_csid} of {cs_id} has no hg_cs_id")
                    })
            })
            .collect::<Result<_>>()?;

        let files = files_by_cs
            .remove(&cs_id)
            .ok_or_else(|| anyhow!("finalize for {cs_id}: changed files were not computed"))?;

        let subtree_change_sources =
            get_subtree_change_sources(ctx, derivation, &bonsai, &HashMap::new()).await?;
        let subtree_changes = HgSubtreeChanges::from_bonsai_subtree_changes(
            bonsai.subtree_changes(),
            subtree_change_sources,
        )?;

        let (hg_cs_id, _hg_blob_cs) = generate_hg_changeset(
            ctx,
            blobstore,
            bonsai,
            manifest_id,
            parent_hg_cs_ids,
            files,
            subtree_changes,
            &derivation_opts,
        )
        .await?;

        results.insert(
            cs_id,
            HgStageOutput {
                entry: Some(root_manifest_entry),
                hg_cs_id: Some(hg_cs_id),
            },
        );
    }

    Ok(results)
}

fn stage_output_to_thrift(
    output: &HgStageOutput,
) -> Result<mercurial_thrift::HgManifestStageOutput> {
    if let Some(hg_cs_id) = output.hg_cs_id {
        let manifest_id = output
            .entry
            .as_ref()
            .and_then(untraced_manifest_id)
            .ok_or_else(|| {
                anyhow!(
                    "terminal stage output with hg_cs_id {hg_cs_id} is missing a tree manifest entry",
                )
            })?;
        return Ok(mercurial_thrift::HgManifestStageOutput::terminal(
            mercurial_thrift::HgManifestStageTerminal {
                hg_cs_id: hg_cs_id.into_nodehash().into_thrift(),
                manifest_id: manifest_id.into_nodehash().into_thrift(),
            },
        ));
    }

    Ok(match &output.entry {
        Some(Entry::Tree(t)) => {
            mercurial_thrift::HgManifestStageOutput::tree(mercurial_thrift::HgManifestStageTree {
                manifest_id: t.untraced().into_nodehash().into_thrift(),
            })
        }
        Some(Entry::Leaf(l)) => {
            let (file_type, filenode_id) = *l.untraced();
            mercurial_thrift::HgManifestStageOutput::leaf(mercurial_thrift::HgManifestStageLeaf {
                file_type: file_type.into_thrift(),
                filenode_id: filenode_id.into_nodehash().into_thrift(),
            })
        }
        None => mercurial_thrift::HgManifestStageOutput::empty(
            mercurial_thrift::HgManifestStageOutputEmpty {},
        ),
    })
}

fn stage_output_from_thrift(
    cs_id: ChangesetId,
    thrift_output: mercurial_thrift::HgManifestStageOutput,
) -> Result<HgStageOutput> {
    match thrift_output {
        mercurial_thrift::HgManifestStageOutput::tree(t) => {
            let manifest_id = HgManifestId::new(HgNodeHash::from_thrift(t.manifest_id)?);
            Ok(HgStageOutput {
                entry: Some(Entry::Tree(Traced::generate(manifest_id))),
                hg_cs_id: None,
            })
        }
        mercurial_thrift::HgManifestStageOutput::leaf(l) => {
            let file_type = FileType::from_thrift(l.file_type)?;
            let filenode_id = HgFileNodeId::from_thrift(l.filenode_id)?;
            Ok(HgStageOutput {
                entry: Some(Entry::Leaf(Traced::generate((file_type, filenode_id)))),
                hg_cs_id: None,
            })
        }
        mercurial_thrift::HgManifestStageOutput::empty(_) => Ok(HgStageOutput {
            entry: None,
            hg_cs_id: None,
        }),
        mercurial_thrift::HgManifestStageOutput::terminal(t) => {
            let hg_cs_id = HgChangesetId::new(HgNodeHash::from_thrift(t.hg_cs_id)?);
            let manifest_id = HgManifestId::new(HgNodeHash::from_thrift(t.manifest_id)?);
            Ok(HgStageOutput {
                entry: Some(Entry::Tree(Traced::generate(manifest_id))),
                hg_cs_id: Some(hg_cs_id),
            })
        }
        mercurial_thrift::HgManifestStageOutput::UnknownField(x) => Err(anyhow!(
            "unknown HgManifestStageOutput variant {x} for {cs_id}",
        )),
    }
}
