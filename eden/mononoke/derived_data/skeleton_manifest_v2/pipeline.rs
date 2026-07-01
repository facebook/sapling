/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use blobstore::Loadable;
use blobstore::Storable;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
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
use mononoke_types::BlobstoreBytes;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::PipelineDerivableVariant;
use mononoke_types::ThriftConvert;
use mononoke_types::path::MPath;
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2;
use mononoke_types::thrift::skeleton_manifest as skmf_thrift;

use crate::RootSkeletonManifestV2Id;
use crate::derive::derive_skeleton_manifest_v2_entry;
use crate::derive::get_file_changes;
use crate::derive::get_skeleton_manifest_subtree_changes;
use crate::format_key;

fn stage_blobstore_key(stage_path: &MPath, key_prefix: &str, cs_id: ChangesetId) -> String {
    format!(
        "derived_skeleton_manifest_v2_stage.{}.{}{}",
        stage_path.get_path_hash().to_hex(),
        key_prefix,
        cs_id,
    )
}

#[async_trait]
impl PipelineDerivable for RootSkeletonManifestV2Id {
    const PIPELINE_DERIVABLE_VARIANT: PipelineDerivableVariant =
        PipelineDerivableVariant::SkeletonManifestsV2;

    const HAS_FINALIZE: bool = false;

    type StageOutput = Option<Entry<SkeletonManifestV2, ()>>;

    async fn derive_stage_batch(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
        payload: &DerivationStagePayload,
        parents: HashMap<ChangesetId, Self::StageOutput>,
        dependency_outputs: HashMap<ChangesetId, HashMap<MPath, Self::StageOutput>>,
    ) -> Result<HashMap<ChangesetId, Self::StageOutput>> {
        let DerivationStagePayload::Manifest(payload) = payload else {
            anyhow::bail!("{} has no finalize derive", Self::NAME);
        };
        let stage_path = &payload.path;

        let mut results = HashMap::new();

        for bonsai in bonsais {
            let cs_id = bonsai.get_changeset_id();
            let all_changes = get_file_changes(&bonsai);

            // Build parent entries for this changeset.
            let parent_entries: Vec<Entry<SkeletonManifestV2, ()>> = bonsai
                .parents()
                .map(|parent_csid| {
                    let output = results
                        .get(&parent_csid)
                        .or_else(|| parents.get(&parent_csid))
                        .ok_or_else(|| anyhow!("missing stage output for parent {parent_csid}"))?;
                    Ok(output.clone())
                })
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect();

            // Build known_entries from dependency stage outputs, keyed by the
            // absolute dependency path.
            let known_entries: HashMap<MPath, Option<Entry<SkeletonManifestV2, ()>>> =
                dependency_outputs
                    .get(&cs_id)
                    .map(|deps| {
                        deps.iter()
                            .map(|(dep_path, dep_output)| (dep_path.clone(), dep_output.clone()))
                            .collect()
                    })
                    .unwrap_or_default();

            let subtree_changes =
                get_skeleton_manifest_subtree_changes(ctx, derivation, None, &bonsai).await?;

            let entry = derive_skeleton_manifest_v2_entry(
                ctx,
                derivation,
                parent_entries,
                all_changes,
                subtree_changes,
                known_entries,
                stage_path.clone(),
            )
            .await?;

            results.insert(cs_id, entry);
        }

        Ok(results)
    }

    async fn extract_stage_output_from_derived(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        derived: &RootSkeletonManifestV2Id,
        stage: &StageId,
    ) -> Result<Self::StageOutput> {
        let StageId::Manifest(stage_path) = stage else {
            anyhow::bail!("{} has no finalize stage", Self::NAME);
        };
        let root = derived.inner_id().load(ctx, derivation.blobstore()).await?;
        Ok(root
            .find_entry(
                ctx.clone(),
                derivation.blobstore().clone(),
                stage_path.clone(),
            )
            .await?)
    }

    async fn store_stage_outputs(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        stage: &StageId,
        outputs: HashMap<ChangesetId, Self::StageOutput>,
    ) -> Result<()> {
        let StageId::Manifest(stage_path) = stage else {
            anyhow::bail!("{} has no finalize stage", Self::NAME);
        };
        let use_normal_mapping = stage_path.is_root()
            && justknobs::eval(
                "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
                None,
                Some("skeleton_manifest_v2"),
            );

        let key_prefix = derivation.mapping_key_prefix::<RootSkeletonManifestV2Id>();
        stream::iter(outputs.into_iter().map(|(cs_id, output)| async move {
            if use_normal_mapping {
                let Some(Entry::Tree(manifest)) = output else {
                    return Err(anyhow!(
                        "terminal stage output for {cs_id} should be Some(Entry::Tree), got {output:?}",
                    ));
                };
                let mf_id = manifest
                    .into_blob()
                    .store(ctx, derivation.blobstore())
                    .await?;
                let key = format_key(derivation, cs_id);
                derivation
                    .blobstore()
                    .put(ctx, key, RootSkeletonManifestV2Id(mf_id).into())
                    .await
            } else {
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                let thrift_output = match output {
                    Some(Entry::Tree(manifest)) => {
                        skmf_thrift::SkeletonManifestV2StageOutput::directory(
                            manifest.into_thrift(),
                        )
                    }
                    Some(Entry::Leaf(())) => skmf_thrift::SkeletonManifestV2StageOutput::file(
                        skmf_thrift::SkeletonManifestV2File {},
                    ),
                    None => skmf_thrift::SkeletonManifestV2StageOutput::empty(
                        skmf_thrift::SkeletonManifestV2StageOutputEmpty {
                            ..Default::default()
                        },
                    ),
                };
                let bytes = compact_protocol::serialize(&thrift_output);
                derivation
                    .blobstore()
                    .put(ctx, key, BlobstoreBytes::from_bytes(bytes))
                    .await
            }
        }))
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await?;
        Ok(())
    }

    async fn fetch_stage_outputs(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        stage: &StageId,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::StageOutput>> {
        let StageId::Manifest(stage_path) = stage else {
            anyhow::bail!("{} has no finalize stage", Self::NAME);
        };
        let use_normal_mapping = stage_path.is_root()
            && justknobs::eval(
                "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
                None,
                Some("skeleton_manifest_v2"),
            );

        let key_prefix = derivation.mapping_key_prefix::<RootSkeletonManifestV2Id>();
        let results = stream::iter(cs_ids.into_iter().map(|cs_id| async move {
            if use_normal_mapping {
                let key = format_key(derivation, cs_id);
                let Some(blob_data) = derivation.blobstore().get(ctx, &key).await? else {
                    return Ok::<_, Error>(None);
                };
                let root: RootSkeletonManifestV2Id = blob_data.try_into()?;
                let manifest = root.inner_id().load(ctx, derivation.blobstore()).await?;
                Ok(Some((cs_id, Some(Entry::Tree(manifest)))))
            } else {
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                let maybe_bytes = derivation.blobstore().get(ctx, &key).await?;
                match maybe_bytes {
                    None => Ok::<_, Error>(None),
                    Some(blob_data) => {
                        let thrift_output: skmf_thrift::SkeletonManifestV2StageOutput =
                            compact_protocol::deserialize(blob_data.into_raw_bytes())?;
                        let output = match thrift_output {
                            skmf_thrift::SkeletonManifestV2StageOutput::directory(mf) => {
                                Some(Entry::Tree(SkeletonManifestV2::from_thrift(mf)?))
                            }
                            skmf_thrift::SkeletonManifestV2StageOutput::file(_) => {
                                Some(Entry::Leaf(()))
                            }
                            skmf_thrift::SkeletonManifestV2StageOutput::empty(_) => None,
                            skmf_thrift::SkeletonManifestV2StageOutput::UnknownField(x) => {
                                return Err(anyhow!(
                                    "unknown SkeletonManifestV2StageOutput variant {x} for {cs_id}"
                                ));
                            }
                        };
                        Ok(Some((cs_id, output)))
                    }
                }
            }
        }))
        .buffered(100)
        .try_filter_map(|opt| async move { Ok(opt) })
        .try_collect::<HashMap<_, _>>()
        .await?;
        Ok(results)
    }
}
