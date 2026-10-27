/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use blobstore::Loadable;
use blobstore::Storable;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use derived_data_manager::ManifestDerivable;
use derived_data_manager::ManifestDerivation;
use fbthrift::compact_protocol;
use manifest::Entry;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::PipelineDerivableVariant;
use mononoke_types::ThriftConvert;
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2;
use mononoke_types::thrift::skeleton_manifest as skmf_thrift;

use crate::RootSkeletonManifestV2Id;
use crate::derive::derive_skeleton_manifest_v2_entry;
use crate::derive::get_file_changes;

#[async_trait]
impl ManifestDerivable for RootSkeletonManifestV2Id {
    type TreeId = SkeletonManifestV2;
    type Leaf = ();

    const PIPELINE_VARIANT: PipelineDerivableVariant =
        PipelineDerivableVariant::SkeletonManifestsV2;
    const STAGE_KEY_PREFIX: &'static str = "derived_skeleton_manifest_v2_stage";
    const MAPPING_KNOB_SWITCH: &'static str = "skeleton_manifest_v2";

    async fn load_root_manifest(
        &self,
        ctx: &CoreContext,
        derivation: &DerivationContext,
    ) -> Result<SkeletonManifestV2> {
        Ok(self.inner_id().load(ctx, derivation.blobstore()).await?)
    }

    async fn from_root_manifest(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        root: SkeletonManifestV2,
    ) -> Result<Self> {
        Ok(Self(
            root.into_blob().store(ctx, derivation.blobstore()).await?,
        ))
    }

    async fn derive_manifest(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        bonsai: BonsaiChangeset,
        input: ManifestDerivation<SkeletonManifestV2, ()>,
    ) -> Result<Option<Entry<SkeletonManifestV2, ()>>> {
        derive_skeleton_manifest_v2_entry(ctx, derivation, input, get_file_changes(&bonsai)).await
    }

    fn encode_entry(entry: Option<Entry<SkeletonManifestV2, ()>>) -> BlobstoreBytes {
        let entry = match entry {
            Some(Entry::Tree(manifest)) => {
                skmf_thrift::SkeletonManifestV2StageOutput::directory(manifest.into_thrift())
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
        BlobstoreBytes::from_bytes(compact_protocol::serialize(&entry))
    }

    fn decode_entry(bytes: BlobstoreBytes) -> Result<Option<Entry<SkeletonManifestV2, ()>>> {
        let thrift_output: skmf_thrift::SkeletonManifestV2StageOutput =
            compact_protocol::deserialize(bytes.into_bytes())?;
        Ok(match thrift_output {
            skmf_thrift::SkeletonManifestV2StageOutput::directory(mf) => {
                Some(Entry::Tree(SkeletonManifestV2::from_thrift(mf)?))
            }
            skmf_thrift::SkeletonManifestV2StageOutput::file(_) => Some(Entry::Leaf(())),
            skmf_thrift::SkeletonManifestV2StageOutput::empty(_) => None,
            skmf_thrift::SkeletonManifestV2StageOutput::UnknownField(x) => {
                return Err(anyhow!("unknown SkeletonManifestV2StageOutput variant {x}"));
            }
        })
    }
}
