/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_git_mapping::BonsaisOrGitShas;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use mononoke_types::Blob;
use mononoke_types::BlobstoreKey;
use mononoke_types::BlobstoreValue;
use mononoke_types::ChangesetId;
use mononoke_types::MononokeId;
use mononoke_types::ThriftConvert;
use mononoke_types::hash::Blake2;
use mononoke_types::impl_typed_hash;
use mononoke_types::typed_hash::IdContext;

use crate::BaseObject;
use crate::GitPackfileBaseItem;
use crate::delta_manifest_v3::GDMV3Entry;
use crate::fetch_non_blob_git_object_bytes;
use crate::thrift;

#[derive(ThriftConvert, Clone, Debug, Eq, PartialEq, Hash)]
#[thrift(thrift::CompactedGitDeltaManifest)]
pub struct CompactedGitDeltaManifest {
    pub entries: Vec<GDMV3Entry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CGDMCommitPackfileItems {
    pub commit_packfile_items: Vec<GitPackfileBaseItem>,
}

impl ThriftConvert for CGDMCommitPackfileItems {
    const NAME: &'static str = "CGDMCommitPackfileItems";
    type Thrift = thrift::CGDMCommitPackfileItems;

    fn from_thrift(thrift: Self::Thrift) -> Result<Self> {
        Ok(Self {
            commit_packfile_items: thrift
                .commit_packfile_items
                .into_iter()
                .map(|item| item.try_into())
                .collect::<Result<_>>()?,
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        thrift::CGDMCommitPackfileItems {
            commit_packfile_items: self
                .commit_packfile_items
                .into_iter()
                .map(|item| item.into())
                .collect(),
            ..Default::default()
        }
    }
}

impl CGDMCommitPackfileItems {
    pub async fn new(
        ctx: &CoreContext,
        blobstore: Arc<dyn Blobstore>,
        bonsai_git_mapping: Arc<dyn BonsaiGitMapping>,
        cs_ids: &[ChangesetId],
    ) -> Result<Self> {
        let git_object_ids = bonsai_git_mapping
            .get(ctx, BonsaisOrGitShas::Bonsai(cs_ids.to_vec()))
            .await
            .context("Failed to fetch bonsai_git_mapping when creating CGDMCommitPackfileItems")?
            .into_iter()
            .map(|entry| entry.git_sha1.to_object_id())
            .collect::<Result<Vec<_>>>()
            .context("Error while converting Git Sha1 to Git Object Id when creating CGDMCommitPackfileItems")?;

        let commit_packfile_items = stream::iter(git_object_ids)
            .map(async |git_object_id| {
                let bytes =
                    fetch_non_blob_git_object_bytes(ctx, &blobstore, &git_object_id).await?;

                let packfile_base_item = GitPackfileBaseItem::try_from(BaseObject::new(bytes)?)?;

                anyhow::Ok(packfile_base_item)
            })
            .buffered(1024)
            .try_collect::<Vec<_>>()
            .await?;

        Ok(Self {
            commit_packfile_items,
        })
    }
}

impl CompactedGitDeltaManifest {
    pub fn new(entries: Vec<GDMV3Entry>) -> Self {
        Self { entries }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct CGDMComponents {
    pub changeset_to_component_id: HashMap<ChangesetId, u64>,
    pub components: HashMap<u64, ComponentInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ComponentInfo {
    pub total_inlined_size: u64,
    pub changeset_count: u64,
    pub cgdm_id: Option<CompactedGitDeltaManifestId>,
    pub cgdm_commits_id: Option<CGDMCommitPackfileItemsId>,
}

impl ThriftConvert for CGDMComponents {
    const NAME: &'static str = "CGDMComponents";
    type Thrift = thrift::CGDMComponents;

    fn from_thrift(thrift: Self::Thrift) -> Result<Self> {
        Ok(Self {
            changeset_to_component_id: thrift
                .component_mappings
                .into_iter()
                .map(|mapping| {
                    Ok((
                        ChangesetId::from_thrift(mapping.cs_id)?,
                        mapping.component_id as u64,
                    ))
                })
                .collect::<Result<_>>()?,
            components: thrift
                .components
                .into_iter()
                .map(|component| {
                    Ok((
                        component.component_id as u64,
                        ComponentInfo {
                            total_inlined_size: component.total_inlined_size as u64,
                            changeset_count: component.changeset_count as u64,
                            cgdm_id: component
                                .cgdm_id
                                .map(CompactedGitDeltaManifestId::from_thrift)
                                .transpose()?,
                            cgdm_commits_id: component
                                .cgdm_commits_id
                                .map(CGDMCommitPackfileItemsId::from_thrift)
                                .transpose()?,
                        },
                    ))
                })
                .collect::<Result<_>>()?,
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        thrift::CGDMComponents {
            component_mappings: self
                .changeset_to_component_id
                .into_iter()
                .map(|(cs_id, component_id)| thrift::ComponentMapping {
                    cs_id: cs_id.into_thrift(),
                    component_id: component_id as i64,
                    ..Default::default()
                })
                .collect(),
            components: self
                .components
                .into_iter()
                .map(|(component_id, component)| thrift::ComponentInfo {
                    component_id: component_id as i64,
                    total_inlined_size: component.total_inlined_size as i64,
                    changeset_count: component.changeset_count as i64,
                    cgdm_id: component.cgdm_id.map(|id| id.into_thrift()),
                    cgdm_commits_id: component.cgdm_commits_id.map(|id| id.into_thrift()),
                    ..Default::default()
                })
                .collect(),

            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct CompactedGitDeltaManifestId(Blake2);

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct CGDMCommitPackfileItemsId(Blake2);

impl_typed_hash! {
    hash_type => CompactedGitDeltaManifestId,
    thrift_hash_type => thrift::CompactedGitDeltaManifestId,
    value_type => CompactedGitDeltaManifest,
    context_type => CompactedGitDeltaManifestIdContext,
    context_key => "cgdm",
}

impl_typed_hash! {
    hash_type => CGDMCommitPackfileItemsId,
    thrift_hash_type => thrift::CGDMCommitPackfileItemsId,
    value_type => CGDMCommitPackfileItems,
    context_type => CGDMCommitPackfileItemsIdContext,
    context_key => "cgdm_commits",
}

impl BlobstoreValue for CompactedGitDeltaManifest {
    type Key = CompactedGitDeltaManifestId;

    fn into_blob(self) -> Blob<Self::Key> {
        let data = self.into_bytes();
        let id = CompactedGitDeltaManifestIdContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}

impl BlobstoreValue for CGDMCommitPackfileItems {
    type Key = CGDMCommitPackfileItemsId;

    fn into_blob(self) -> Blob<Self::Key> {
        let data = self.into_bytes();
        let id = CGDMCommitPackfileItemsIdContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}
