/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

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
use fastlog::RootFastlog;
use filenodes_derivation::FilenodesOnlyPublic;
use fsnodes::RootFsnodeId;
use futures::stream;
use futures::stream::StreamExt;
use git_types::MappedGitCommitId;
use git_types::RootGitDeltaManifestId;
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
    ) -> impl std::future::Future<Output = Result<Duration, DerivationError>> + Send;
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
    /// The provided batch of csids must be in topological
    /// order.
    ///
    /// The caller must have arranged for the dependencies
    /// and ancestors of the batch to have already been derived for all the derived
    /// datat types requested.
    ///
    /// If any dependency or ancestor is not already derived, an error
    /// will be returned.
    /// If a dependent derived data type has not been derived for the batch of csids prior to
    /// this, it will be derived first. The same pre-conditions apply on the dependent derived data
    /// type.
    fn derive_bulk(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_types: &[DerivableType],
    ) -> impl std::future::Future<Output = Result<Duration, DerivationError>> + Send {
        // Note: We could skip the ones that are dependent on others that are present in this list to
        // avoid racing with ourselves
        stream::iter(derived_data_types)
            .then(move |derived_data_type| {
                cloned!(csids, rederivation);
                async move {
                    match derived_data_type {
                        DerivableType::Unodes => {
                            self.derive_exactly_batch::<RootUnodeManifestId>(
                                ctx,
                                csids,
                                rederivation,
                            )
                            .await
                        }
                        DerivableType::BlameV2 => {
                            self.derive_exactly_batch::<RootBlameV2>(ctx, csids, rederivation)
                                .await
                        }
                        DerivableType::FileNodes => {
                            self.derive_exactly_batch::<FilenodesOnlyPublic>(
                                ctx,
                                csids,
                                rederivation,
                            )
                            .await
                        }
                        DerivableType::HgChangesets => {
                            self.derive_exactly_batch::<MappedHgChangesetId>(
                                ctx,
                                csids,
                                rederivation,
                            )
                            .await
                        }
                        DerivableType::HgAugmentedManifests => {
                            self.derive_exactly_batch::<RootHgAugmentedManifestId>(
                                ctx,
                                csids,
                                rederivation,
                            )
                            .await
                        }
                        DerivableType::Fsnodes => {
                            self.derive_exactly_batch::<RootFsnodeId>(ctx, csids, rederivation)
                                .await
                        }
                        DerivableType::Fastlog => {
                            self.derive_exactly_batch::<RootFastlog>(ctx, csids, rederivation)
                                .await
                        }
                        DerivableType::DeletedManifests => {
                            self.derive_exactly_batch::<RootDeletedManifestV2Id>(
                                ctx,
                                csids,
                                rederivation,
                            )
                            .await
                        }
                        DerivableType::SkeletonManifests => {
                            self.derive_exactly_batch::<RootSkeletonManifestId>(
                                ctx,
                                csids,
                                rederivation,
                            )
                            .await
                        }
                        DerivableType::ChangesetInfo => {
                            self.derive_exactly_batch::<ChangesetInfo>(ctx, csids, rederivation)
                                .await
                        }
                        DerivableType::GitTrees => {
                            self.derive_exactly_batch::<TreeHandle>(ctx, csids, rederivation)
                                .await
                        }
                        DerivableType::GitCommits => {
                            self.derive_exactly_batch::<MappedGitCommitId>(ctx, csids, rederivation)
                                .await
                        }
                        DerivableType::GitDeltaManifests => {
                            self.derive_exactly_batch::<RootGitDeltaManifestId>(
                                ctx,
                                csids,
                                rederivation,
                            )
                            .await
                        }
                        DerivableType::BssmV3 => {
                            self.derive_exactly_batch::<RootBssmV3DirectoryId>(
                                ctx,
                                csids,
                                rederivation,
                            )
                            .await
                        }
                        DerivableType::TestManifests => {
                            self.derive_exactly_batch::<RootTestManifestDirectory>(
                                ctx,
                                csids,
                                rederivation,
                            )
                            .await
                        }
                        DerivableType::TestShardedManifests => {
                            self.derive_exactly_batch::<RootTestShardedManifestDirectory>(
                                ctx,
                                csids,
                                rederivation,
                            )
                            .await
                        }
                    }
                }
            })
            .fold(Ok(Duration::ZERO), |acc, x| async move {
                match (acc, x) {
                    (Ok(duration), Ok(acc)) => Ok(duration + acc),
                    (Err(e), _) | (_, Err(e)) => Err(e),
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
