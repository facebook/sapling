/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

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
use fastlog::RootFastlog;
use filenodes_derivation::FilenodesOnlyPublic;
use fsnodes::RootFsnodeId;
use futures::stream;
use futures::stream::StreamExt;
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

pub trait BulkDerivation {
    fn derive_bulk(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_types: &[DerivableType],
        override_batch_size: Option<u64>,
    ) -> impl std::future::Future<Output = Result<(), SharedDerivationError>> + Send;
    fn is_derived(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> impl std::future::Future<Output = Result<bool, DerivationError>> + Send;
}

impl BulkDerivation for DerivedDataManager {
    /// Derive all the desired derived data types for all the desired csids
    ///
    /// If the dependent types or changesets are not derived yet, they will be derived now
    fn derive_bulk(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_types: &[DerivableType],
        override_batch_size: Option<u64>,
    ) -> impl std::future::Future<Output = Result<(), SharedDerivationError>> + Send {
        // Note: We could skip the ones that are dependent on others that are present in this list to
        // avoid racing with ourselves
        stream::iter(derived_data_types)
            .then(move |derived_data_type| {
                cloned!(csids, rederivation, override_batch_size);
                async move {
                    let csids = &csids;
                    let derivation_ctx = &self.derivation_context(rederivation);
                    match derived_data_type {
                        DerivableType::Unodes => {
                            self.derive_heads::<RootUnodeManifestId>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::BlameV2 => {
                            self.derive_heads::<RootBlameV2>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::FileNodes => {
                            self.derive_heads::<FilenodesOnlyPublic>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::HgChangesets => {
                            self.derive_heads::<MappedHgChangesetId>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::HgAugmentedManifests => {
                            self.derive_heads::<RootHgAugmentedManifestId>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::Fsnodes => {
                            self.derive_heads::<RootFsnodeId>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::Fastlog => {
                            self.derive_heads::<RootFastlog>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::DeletedManifests => {
                            self.derive_heads::<RootDeletedManifestV2Id>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::SkeletonManifests => {
                            self.derive_heads::<RootSkeletonManifestId>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::ChangesetInfo => {
                            self.derive_heads::<ChangesetInfo>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::GitTrees => {
                            self.derive_heads::<TreeHandle>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::GitCommits => {
                            self.derive_heads::<MappedGitCommitId>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::GitDeltaManifests => {
                            self.derive_heads::<RootGitDeltaManifestId>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::GitDeltaManifestsV2 => {
                            self.derive_heads::<RootGitDeltaManifestV2Id>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::BssmV3 => {
                            self.derive_heads::<RootBssmV3DirectoryId>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::TestManifests => {
                            self.derive_heads::<RootTestManifestDirectory>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                        DerivableType::TestShardedManifests => {
                            self.derive_heads::<RootTestShardedManifestDirectory>(
                                ctx,
                                derivation_ctx,
                                csids,
                                override_batch_size,
                            )
                            .await
                        }
                    }
                }
            })
            .fold(Ok(()), |acc, x| async move {
                match (acc, x) {
                    (Err(e), _) | (_, Err(e)) => Err(e),
                    _ => Ok(()),
                }
            })
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
