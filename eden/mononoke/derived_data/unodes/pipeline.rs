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
use context::CoreContext;
use derived_data_manager::DerivationContext;
use derived_data_manager::DerivationStagePayload;
use derived_data_manager::PipelineDerivable;
use fbthrift::compact_protocol;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::ManifestUnodeId;
use mononoke_types::PipelineDerivableVariant;
use mononoke_types::path::MPath;
use mononoke_types::thrift::unodes as unodes_thrift;

use crate::RootUnodeManifestId;
use crate::derive::derive_unode_entry;
use crate::mapping::format_key;
use crate::mapping::get_file_changes;

fn stage_blobstore_key(stage_path: &MPath, key_prefix: &str, cs_id: ChangesetId) -> String {
    format!(
        "derived_unode_stage.{}.{}{}",
        stage_path.get_path_hash().to_hex(),
        key_prefix,
        cs_id,
    )
}

#[async_trait]
impl PipelineDerivable for RootUnodeManifestId {
    const PIPELINE_DERIVABLE_VARIANT: PipelineDerivableVariant = PipelineDerivableVariant::Unodes;

    type StageOutput = Option<Entry<ManifestUnodeId, FileUnodeId>>;

    async fn derive_stage_batch(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
        payload: &DerivationStagePayload,
        parents: HashMap<ChangesetId, Self::StageOutput>,
        dependency_outputs: HashMap<ChangesetId, HashMap<MPath, Self::StageOutput>>,
    ) -> Result<HashMap<ChangesetId, Self::StageOutput>> {
        let DerivationStagePayload::Manifest(payload) = payload;
        let stage_path = &payload.path;

        let mut results = HashMap::new();

        for bonsai in bonsais {
            let cs_id = bonsai.get_changeset_id();
            let mut all_changes = get_file_changes(&bonsai);

            // Build parent entries for this changeset.
            let parent_entries: Vec<Entry<ManifestUnodeId, FileUnodeId>> = bonsai
                .parents()
                .map(|parent_csid| {
                    let output = results
                        .get(&parent_csid)
                        .or_else(|| parents.get(&parent_csid))
                        .ok_or_else(|| {
                            anyhow!("missing stage output for parent {}", parent_csid)
                        })?;
                    Ok(output.clone())
                })
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect();

            // Build known_entries from dependency stage outputs. The manager
            // keys dependency_outputs by absolute dep path, so we can copy
            // entries straight in.
            let known_entries: HashMap<MPath, Option<Entry<ManifestUnodeId, FileUnodeId>>> =
                dependency_outputs
                    .get(&cs_id)
                    .map(|deps| {
                        deps.iter()
                            .map(|(dep_path, dep_output)| (dep_path.clone(), dep_output.clone()))
                            .collect()
                    })
                    .unwrap_or_default();

            let (mut additional_changes, manifest_replacements) =
                crate::derive::get_unode_subtree_changes(
                    ctx,
                    derivation,
                    None,
                    bonsai.subtree_changes(),
                    &all_changes,
                )
                .await?;
            all_changes.append(&mut additional_changes);

            let entry = derive_unode_entry(
                ctx,
                derivation,
                cs_id,
                parent_entries,
                all_changes,
                manifest_replacements,
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
        derived: &RootUnodeManifestId,
        stage_path: &MPath,
    ) -> Result<Self::StageOutput> {
        Ok(derived
            .manifest_unode_id()
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
        stage_path: &MPath,
        outputs: HashMap<ChangesetId, Self::StageOutput>,
    ) -> Result<()> {
        let use_normal_mapping = stage_path.is_root()
            && justknobs::eval(
                "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
                None,
                Some("unodes"),
            );

        let key_prefix = derivation.mapping_key_prefix::<RootUnodeManifestId>();
        stream::iter(outputs.into_iter().map(|(cs_id, output)| async move {
            if use_normal_mapping {
                let Some(Entry::Tree(mf_unode_id)) = output else {
                    return Err(anyhow!(
                        "terminal stage output for {} should be Some(Entry::Tree), got {:?}",
                        cs_id,
                        output,
                    ));
                };
                let key = format_key(derivation, cs_id);
                derivation
                    .blobstore()
                    .put(ctx, key, RootUnodeManifestId(mf_unode_id).into())
                    .await
            } else {
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                let thrift_output = match output {
                    Some(Entry::Tree(mf_unode_id)) => {
                        unodes_thrift::UnodeStageOutput::manifest_unode_id(
                            mf_unode_id.into_thrift(),
                        )
                    }
                    Some(Entry::Leaf(file_unode_id)) => {
                        unodes_thrift::UnodeStageOutput::file_unode_id(file_unode_id.into_thrift())
                    }
                    None => unodes_thrift::UnodeStageOutput::empty(
                        unodes_thrift::UnodeStageOutputEmpty {
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
        stage_path: &MPath,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::StageOutput>> {
        let use_normal_mapping = stage_path.is_root()
            && justknobs::eval(
                "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
                None,
                Some("unodes"),
            );

        let key_prefix = derivation.mapping_key_prefix::<RootUnodeManifestId>();
        let results = stream::iter(cs_ids.into_iter().map(|cs_id| async move {
            if use_normal_mapping {
                let key = format_key(derivation, cs_id);
                let Some(blob_data) = derivation.blobstore().get(ctx, &key).await? else {
                    return Ok::<_, Error>(None);
                };
                let root: RootUnodeManifestId = blob_data.try_into()?;
                Ok(Some((cs_id, Some(Entry::Tree(*root.manifest_unode_id())))))
            } else {
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                let maybe_bytes = derivation.blobstore().get(ctx, &key).await?;
                match maybe_bytes {
                    None => Ok::<_, Error>(None),
                    Some(blob_data) => {
                        let thrift_output: unodes_thrift::UnodeStageOutput =
                            compact_protocol::deserialize(blob_data.into_raw_bytes())?;
                        let output = match thrift_output {
                            unodes_thrift::UnodeStageOutput::manifest_unode_id(id) => {
                                Some(Entry::Tree(ManifestUnodeId::from_thrift(id)?))
                            }
                            unodes_thrift::UnodeStageOutput::file_unode_id(id) => {
                                Some(Entry::Leaf(FileUnodeId::from_thrift(id)?))
                            }
                            unodes_thrift::UnodeStageOutput::empty(_) => None,
                            unodes_thrift::UnodeStageOutput::UnknownField(x) => {
                                return Err(anyhow!(
                                    "unknown UnodeStageOutput variant {} for {}",
                                    x,
                                    cs_id
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
