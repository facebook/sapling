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
use mononoke_types::FsnodeId;
use mononoke_types::PipelineDerivableVariant;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::path::MPath;
use mononoke_types::thrift::fsnodes as fsnodes_thrift;

use crate::derive::derive_fsnode_entry;
use crate::mapping::RootFsnodeId;
use crate::mapping::get_file_changes;

#[async_trait]
impl PipelineDerivable for RootFsnodeId {
    const PIPELINE_DERIVABLE_VARIANT: PipelineDerivableVariant = PipelineDerivableVariant::Fsnodes;

    type StageOutput = Option<Entry<FsnodeId, FsnodeFile>>;

    async fn derive_stage_batch(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
        stage: &DerivationPipelineStageConfig,
        parents: HashMap<ChangesetId, Self::StageOutput>,
        dependency_outputs: HashMap<ChangesetId, HashMap<String, Self::StageOutput>>,
    ) -> Result<HashMap<ChangesetId, Self::StageOutput>> {
        let pipeline_config = derivation
            .derivation_pipeline_config()
            .get(&DerivableType::Fsnodes)
            .ok_or_else(|| anyhow!("no derivation pipeline config for fsnodes"))?;

        let stage_path = match &stage.type_config {
            DerivationPipelineStageTypeConfig::Manifest(cfg) => &cfg.path,
        };

        let mut results = HashMap::new();

        for bonsai in bonsais {
            let cs_id = bonsai.get_changeset_id();
            let all_changes = get_file_changes(&bonsai);

            // Build parent FsnodeIds for this changeset.
            let parent_entries: Vec<Entry<FsnodeId, FsnodeFile>> = bonsai
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
            let mut known_entries: HashMap<MPath, Option<Entry<FsnodeId, FsnodeFile>>> =
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

            // Subtree copy operations are not supported with derivation pipeline.
            let subtree_changes = vec![];

            let entry = derive_fsnode_entry(
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
        derived: &RootFsnodeId,
        stage: &DerivationPipelineStageConfig,
    ) -> Result<Self::StageOutput> {
        let stage_path = match &stage.type_config {
            DerivationPipelineStageTypeConfig::Manifest(cfg) => &cfg.path,
        };
        Ok(derived
            .fsnode_id()
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
        let key_prefix = derivation.mapping_key_prefix::<RootFsnodeId>();
        stream::iter(outputs.into_iter().map(|(cs_id, output)| {
            let key = format!("derived_fsnode_stage.{}.{}{}", stage_id, key_prefix, cs_id);
            async move {
                let thrift_output = match output {
                    Some(Entry::Tree(fsnode_id)) => {
                        fsnodes_thrift::FsnodeStageOutput::fsnode_id(fsnode_id.into_thrift())
                    }
                    Some(Entry::Leaf(fsnode_file)) => {
                        fsnodes_thrift::FsnodeStageOutput::fsnode_file(fsnode_file.into_thrift())
                    }
                    None => fsnodes_thrift::FsnodeStageOutput::empty(
                        fsnodes_thrift::FsnodeStageOutputEmpty {
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
        let key_prefix = derivation.mapping_key_prefix::<RootFsnodeId>();
        let results = stream::iter(cs_ids.into_iter().map(|cs_id| {
            let key = format!("derived_fsnode_stage.{}.{}{}", stage_id, key_prefix, cs_id);
            async move {
                let maybe_bytes = derivation.blobstore().get(ctx, &key).await?;
                match maybe_bytes {
                    None => Ok::<_, Error>(None),
                    Some(blob_data) => {
                        let thrift_output: fsnodes_thrift::FsnodeStageOutput =
                            compact_protocol::deserialize(blob_data.into_raw_bytes())?;
                        let output = match thrift_output {
                            fsnodes_thrift::FsnodeStageOutput::fsnode_id(id) => {
                                Some(Entry::Tree(FsnodeId::from_thrift(id)?))
                            }
                            fsnodes_thrift::FsnodeStageOutput::fsnode_file(file) => {
                                Some(Entry::Leaf(FsnodeFile::from_thrift(file)?))
                            }
                            fsnodes_thrift::FsnodeStageOutput::empty(_) => None,
                            fsnodes_thrift::FsnodeStageOutput::UnknownField(x) => {
                                return Err(anyhow!(
                                    "unknown FsnodeStageOutput variant {} for {}",
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
