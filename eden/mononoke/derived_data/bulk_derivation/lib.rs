/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use async_trait::async_trait;
use basename_suffix_skeleton_manifest_v3::RootBssmV3DirectoryId;
use blame::RootBlameV2;
use changeset_info::ChangesetInfo;
use cloned::cloned;
use context::CoreContext;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationError;
use derived_data_manager::DerivedDataManager;
use derived_data_manager::Rederivation;
use derived_data_manager::SharedDerivationError;
use derived_data_manager::VisitedDerivableTypesMap;
use fastlog::RootFastlog;
use filenodes_derivation::FilenodesOnlyPublic;
use fsnodes::RootFsnodeId;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use git_types::MappedGitCommitId;
use git_types::RootGitDeltaManifestId;
use git_types::RootGitDeltaManifestV2Id;
use git_types::TreeHandle;
use mercurial_derivation::MappedHgChangesetId;
use mercurial_derivation::RootHgAugmentedManifestId;
use mononoke_types::ChangesetId;
use skeleton_manifest::RootSkeletonManifestId;
use test_manifest::RootTestManifestDirectory;
use test_sharded_manifest::RootTestShardedManifestDirectory;
use unodes::RootUnodeManifestId;

#[async_trait]
pub trait BulkDerivation {
    async fn derive_bulk(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_types: &[DerivableType],
        override_batch_size: Option<u64>,
    ) -> Result<(), SharedDerivationError>;
    async fn is_derived(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<bool, DerivationError>;
}

#[async_trait]
impl BulkDerivation for DerivedDataManager {
    /// Derive all the desired derived data types for all the desired csids
    ///
    /// If the dependent types or changesets are not derived yet, they will be derived now
    async fn derive_bulk(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_types: &[DerivableType],
        override_batch_size: Option<u64>,
    ) -> Result<(), SharedDerivationError> {
        let visited = VisitedDerivableTypesMap::default();
        stream::iter(derived_data_types)
            .map(move |derived_data_type| {
                cloned!(rederivation, visited);
                async move {
                    match derived_data_type {
                        DerivableType::Unodes => {
                            self.derive_heads_with_visited::<RootUnodeManifestId>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::BlameV2 => {
                            self.derive_heads_with_visited::<RootBlameV2>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::FileNodes => {
                            self.derive_heads_with_visited::<FilenodesOnlyPublic>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::HgChangesets => {
                            self.derive_heads_with_visited::<MappedHgChangesetId>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::HgAugmentedManifests => {
                            self.derive_heads_with_visited::<RootHgAugmentedManifestId>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::Fsnodes => {
                            self.derive_heads_with_visited::<RootFsnodeId>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::Fastlog => {
                            self.derive_heads_with_visited::<RootFastlog>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::DeletedManifests => {
                            self.derive_heads_with_visited::<RootDeletedManifestV2Id>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::SkeletonManifests => {
                            self.derive_heads_with_visited::<RootSkeletonManifestId>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::ChangesetInfo => {
                            self.derive_heads_with_visited::<ChangesetInfo>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::GitTrees => {
                            self.derive_heads_with_visited::<TreeHandle>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::GitCommits => {
                            self.derive_heads_with_visited::<MappedGitCommitId>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::GitDeltaManifests => {
                            self.derive_heads_with_visited::<RootGitDeltaManifestId>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::GitDeltaManifestsV2 => {
                            self.derive_heads_with_visited::<RootGitDeltaManifestV2Id>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::BssmV3 => {
                            self.derive_heads_with_visited::<RootBssmV3DirectoryId>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::TestManifests => {
                            self.derive_heads_with_visited::<RootTestManifestDirectory>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                        DerivableType::TestShardedManifests => {
                            self.derive_heads_with_visited::<RootTestShardedManifestDirectory>(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                        }
                    }
                }
            })
            .boxed()
            .buffer_unordered(10)
            .try_collect::<Vec<_>>()
            .await?;

        Ok(())
    }
    async fn is_derived(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<bool, DerivationError> {
        Ok(match derived_data_type {
            DerivableType::Unodes => self
                .fetch_derived::<RootUnodeManifestId>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::BlameV2 => self
                .fetch_derived::<RootBlameV2>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::FileNodes => self
                .fetch_derived::<FilenodesOnlyPublic>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::HgChangesets => self
                .fetch_derived::<MappedHgChangesetId>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::HgAugmentedManifests => self
                .fetch_derived::<RootHgAugmentedManifestId>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::Fsnodes => self
                .fetch_derived::<RootFsnodeId>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::Fastlog => self
                .fetch_derived::<RootFastlog>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::DeletedManifests => self
                .fetch_derived::<RootDeletedManifestV2Id>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::SkeletonManifests => self
                .fetch_derived::<RootSkeletonManifestId>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::ChangesetInfo => self
                .fetch_derived::<ChangesetInfo>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::GitTrees => self
                .fetch_derived::<TreeHandle>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::GitCommits => self
                .fetch_derived::<MappedGitCommitId>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::GitDeltaManifests => self
                .fetch_derived::<RootGitDeltaManifestId>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::GitDeltaManifestsV2 => self
                .fetch_derived::<RootGitDeltaManifestV2Id>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::BssmV3 => self
                .fetch_derived::<RootBssmV3DirectoryId>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::TestManifests => self
                .fetch_derived::<RootTestManifestDirectory>(ctx, csid, rederivation)
                .await?
                .is_some(),
            DerivableType::TestShardedManifests => self
                .fetch_derived::<RootTestShardedManifestDirectory>(ctx, csid, rederivation)
                .await?
                .is_some(),
        })
    }
}
