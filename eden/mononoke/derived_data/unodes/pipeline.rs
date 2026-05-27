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
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_manager::PipelineDerivable;
use fbthrift::compact_protocol;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use manifest::Entry;
use manifest::ManifestOps;
use metaconfig_types::DerivationPipelineStageConfig;
use metaconfig_types::DerivationPipelineStageTypeConfig;
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

#[async_trait]
impl PipelineDerivable for RootUnodeManifestId {
    const PIPELINE_DERIVABLE_VARIANT: PipelineDerivableVariant = PipelineDerivableVariant::Unodes;

    type StageOutput = Option<Entry<ManifestUnodeId, FileUnodeId>>;

    async fn derive_stage_batch(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
        stage: &DerivationPipelineStageConfig,
        _stage_id: &str,
        parents: HashMap<ChangesetId, Self::StageOutput>,
        dependency_outputs: HashMap<ChangesetId, HashMap<String, Self::StageOutput>>,
    ) -> Result<HashMap<ChangesetId, Self::StageOutput>> {
        let pipeline_config = derivation
            .pipeline_config()
            .filter(|cfg| cfg.types.contains(&DerivableType::Unodes))
            .ok_or_else(|| anyhow!("no derivation pipeline config for unodes"))?;

        let stage_path = match &stage.type_config {
            DerivationPipelineStageTypeConfig::Manifest(cfg) => &cfg.path,
        };

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

            // Build known_entries from dependency stage outputs.
            let mut known_entries: HashMap<MPath, Option<Entry<ManifestUnodeId, FileUnodeId>>> =
                HashMap::new();
            if let Some(deps) = dependency_outputs.get(&cs_id) {
                for (dep_stage_id, dep_output) in deps {
                    if let Some(dep_stage_config) = pipeline_config.stages.get(dep_stage_id) {
                        let dep_path = match &dep_stage_config.type_config {
                            DerivationPipelineStageTypeConfig::Manifest(cfg) => cfg.path.clone(),
                        };
                        known_entries.insert(dep_path, dep_output.clone());
                    }
                }
            }

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
        stage: &DerivationPipelineStageConfig,
    ) -> Result<Self::StageOutput> {
        let stage_path = match &stage.type_config {
            DerivationPipelineStageTypeConfig::Manifest(cfg) => &cfg.path,
        };
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
        stage_id: &str,
        outputs: HashMap<ChangesetId, Self::StageOutput>,
    ) -> Result<()> {
        let use_normal_mapping = justknobs::eval(
            "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
            None,
            Some("unodes"),
        ) && derivation
            .pipeline_config()
            .filter(|cfg| cfg.types.contains(&DerivableType::Unodes))
            .and_then(|cfg| cfg.stages.get(stage_id))
            .is_some_and(|stage| stage.terminal);

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
                let key = format!("derived_unode_stage.{}.{}{}", stage_id, key_prefix, cs_id);
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
        stage_id: &str,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::StageOutput>> {
        let use_normal_mapping = justknobs::eval(
            "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
            None,
            Some("unodes"),
        ) && derivation
            .pipeline_config()
            .filter(|cfg| cfg.types.contains(&DerivableType::Unodes))
            .and_then(|cfg| cfg.stages.get(stage_id))
            .is_some_and(|stage| stage.terminal);

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
                let key = format!("derived_unode_stage.{}.{}{}", stage_id, key_prefix, cs_id);
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
