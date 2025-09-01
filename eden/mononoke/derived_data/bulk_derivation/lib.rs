/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use anyhow::Error;
use async_trait::async_trait;
use basename_suffix_skeleton_manifest_v3::RootBssmV3DirectoryId;
use blame::RootBlameV2;
use case_conflict_skeleton_manifest::RootCaseConflictSkeletonManifestId;
use changeset_info::ChangesetInfo;
use cloned::cloned;
use content_manifest_derivation::RootContentManifestId;
use context::CoreContext;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationError;
use derived_data_manager::DerivedDataManager;
use derived_data_manager::Rederivation;
use derived_data_manager::SharedDerivationError;
use derived_data_manager::VisitedDerivableTypesMap;
use derived_data_manager::VisitedDerivableTypesMapStatic;
use fastlog::RootFastlog;
use filenodes_derivation::FilenodesOnlyPublic;
use fsnodes::RootFsnodeId;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use git_types::MappedGitCommitId;
use git_types::RootGitDeltaManifestV2Id;
use git_types::RootGitDeltaManifestV3Id;
use inferred_copy_from::RootInferredCopyFromId;
use itertools::Itertools;
use mercurial_derivation::MappedHgChangesetId;
use mercurial_derivation::RootHgAugmentedManifestId;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use skeleton_manifest::RootSkeletonManifestId;
use skeleton_manifest_v2::RootSkeletonManifestV2Id;
use test_manifest::RootTestManifestDirectory;
use test_sharded_manifest::RootTestShardedManifestDirectory;
use unodes::RootUnodeManifestId;

#[async_trait]
pub trait BulkDerivation {
    /// Derive all the desired derived data types for all the desired csids
    ///
    /// If the dependent types or changesets are not derived yet, they will be derived now
    async fn derive_bulk_locally(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_types: &[DerivableType],
        override_batch_size: Option<u64>,
    ) -> Result<(), SharedDerivationError>;

    /// Derive all the desired derived data types for all the desired csids
    ///
    /// If the dependent types or changesets are not derived yet, they will be derived now
    ///
    /// Perform the derivation remotely using the Derived Data Service, or fall back to local
    /// derivation if necessary
    async fn derive_bulk(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_types: &[DerivableType],
        override_concurrency: Option<usize>,
    ) -> Result<(), SharedDerivationError>;

    /// Derive data for exactly a batch of changesets.
    ///
    /// The provided batch of changesets must be in topological
    /// order. The caller must have arranged for the dependencies
    /// and ancestors of the batch to have already been derived.  If
    /// any dependency or ancestor is not already derived, an error
    /// will be returned.
    async fn derive_exactly_batch(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<(), DerivationError>;

    /// Derive data for exactly all underived changesets in a batch.
    ///
    /// The provided batch of changesets must be in topological
    /// order. The caller must have arranged for the dependencies
    /// and ancestors of the batch to have already been derived. If
    /// any dependency or ancestor is not already derived, an error
    /// will be returned.
    async fn derive_exactly_underived_batch(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<(), DerivationError>;

    /// Check if the given derived data type is derived for the given changeset id.
    async fn is_derived(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<bool, DerivationError>;

    /// Returns a `Vec` that contains all changeset ids that don't have the given
    /// derived data type derived from the given changeset ids.
    async fn pending(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<Vec<ChangesetId>, DerivationError>;

    /// Fetch derived data for a batch of changesets if they have previously
    /// been derived.
    ///
    /// Returns a hashmap from changeset id to the debug format of the derived data.
    /// Changesets for which the data has not previously been derived are omitted.
    async fn fetch_derived_batch(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<HashMap<ChangesetId, String>, DerivationError>;

    /// Returns the number of ancestor of the given changeset that don't have
    /// the given derived data type derived.
    async fn count_underived(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        limit: Option<u64>,
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<u64, DerivationError>;

    /// Derive the given derived data type for the given changeset id, using its
    /// predecessor derived data types.
    async fn derive_from_predecessor(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<(), DerivationError>;
}

struct SingleTypeManager<T: BonsaiDerivable> {
    manager: DerivedDataManager,
    derived_data_type: PhantomData<T>,
}

impl<T: BonsaiDerivable> SingleTypeManager<T> {
    fn new(manager: DerivedDataManager) -> Self {
        Self {
            manager,
            derived_data_type: PhantomData,
        }
    }
}

#[async_trait]
trait SingleTypeDerivation: Send + Sync {
    async fn derive_heads_with_visited(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
        override_batch_size: Option<u64>,
        rederivation: Option<Arc<dyn Rederivation>>,
        visited: VisitedDerivableTypesMapStatic<u64, SharedDerivationError>,
    ) -> Result<(), SharedDerivationError>;

    async fn derive_exactly_batch(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<(), DerivationError>;

    async fn derive_exactly_underived_batch(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<(), DerivationError>;

    async fn is_derived(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<bool, DerivationError>;

    async fn pending(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<Vec<ChangesetId>, DerivationError>;

    async fn fetch_derived_batch(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<HashMap<ChangesetId, String>, DerivationError>;

    async fn count_underived(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        limit: Option<u64>,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<u64, DerivationError>;

    async fn derive_from_predecessor(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<(), DerivationError>;

    async fn derive(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<(), SharedDerivationError>;
}

#[async_trait]
impl<T: BonsaiDerivable> SingleTypeDerivation for SingleTypeManager<T> {
    async fn derive_heads_with_visited(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
        override_batch_size: Option<u64>,
        rederivation: Option<Arc<dyn Rederivation>>,
        visited: VisitedDerivableTypesMapStatic<u64, SharedDerivationError>,
    ) -> Result<(), SharedDerivationError> {
        self.manager
            .clone()
            .derive_heads_with_visited::<T>(ctx, csids, override_batch_size, rederivation, visited)
            .await?;
        Ok(())
    }

    async fn derive_exactly_batch(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<(), DerivationError> {
        self.manager
            .derive_exactly_batch::<T>(ctx, csids.to_vec(), rederivation)
            .await?;
        Ok(())
    }

    async fn derive_exactly_underived_batch(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<(), DerivationError> {
        self.manager
            .derive_exactly_underived_batch::<T>(ctx, csids.to_vec(), rederivation)
            .await?;
        Ok(())
    }

    async fn is_derived(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<bool, DerivationError> {
        Ok(self
            .manager
            .fetch_derived::<T>(ctx, csid, rederivation)
            .await?
            .is_some())
    }

    async fn pending(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<Vec<ChangesetId>, DerivationError> {
        let derived = self
            .manager
            .fetch_derived_batch::<T>(ctx, csids.to_vec(), rederivation)
            .await?;
        Ok(csids
            .iter()
            .filter(|csid| !derived.contains_key(csid))
            .copied()
            .collect())
    }

    async fn fetch_derived_batch(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<HashMap<ChangesetId, String>, DerivationError> {
        let derived = self
            .manager
            .fetch_derived_batch::<T>(ctx, csids.to_vec(), rederivation)
            .await?;
        Ok(derived
            .into_iter()
            .map(|(csid, derived)| (csid, format!("{:?}", derived)))
            .collect())
    }

    async fn count_underived(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        limit: Option<u64>,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<u64, DerivationError> {
        self.manager
            .count_underived::<T>(ctx, csid, limit, rederivation)
            .await
    }

    async fn derive_from_predecessor(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<(), DerivationError> {
        self.manager
            .derive_from_predecessor::<T>(ctx, csid, rederivation)
            .await?;
        Ok(())
    }

    async fn derive(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<(), SharedDerivationError> {
        self.manager.derive::<T>(ctx, csid, rederivation).await?;
        Ok(())
    }
}

fn manager_for_type(
    manager: &DerivedDataManager,
    derived_data_type: DerivableType,
) -> Arc<dyn SingleTypeDerivation + Send + Sync + 'static> {
    let manager = manager.clone();
    match derived_data_type {
        DerivableType::Unodes => Arc::new(SingleTypeManager::<RootUnodeManifestId>::new(manager)),
        DerivableType::BlameV2 => Arc::new(SingleTypeManager::<RootBlameV2>::new(manager)),
        DerivableType::FileNodes => {
            Arc::new(SingleTypeManager::<FilenodesOnlyPublic>::new(manager))
        }
        DerivableType::HgChangesets => {
            Arc::new(SingleTypeManager::<MappedHgChangesetId>::new(manager))
        }
        DerivableType::HgAugmentedManifests => {
            Arc::new(SingleTypeManager::<RootHgAugmentedManifestId>::new(manager))
        }
        DerivableType::Fsnodes => Arc::new(SingleTypeManager::<RootFsnodeId>::new(manager)),
        DerivableType::Fastlog => Arc::new(SingleTypeManager::<RootFastlog>::new(manager)),
        DerivableType::DeletedManifests => {
            Arc::new(SingleTypeManager::<RootDeletedManifestV2Id>::new(manager))
        }
        DerivableType::SkeletonManifests => {
            Arc::new(SingleTypeManager::<RootSkeletonManifestId>::new(manager))
        }
        DerivableType::SkeletonManifestsV2 => {
            Arc::new(SingleTypeManager::<RootSkeletonManifestV2Id>::new(manager))
        }
        DerivableType::Ccsm => {
            Arc::new(SingleTypeManager::<RootCaseConflictSkeletonManifestId>::new(manager))
        }
        DerivableType::ContentManifests => {
            Arc::new(SingleTypeManager::<RootContentManifestId>::new(manager))
        }
        DerivableType::ChangesetInfo => Arc::new(SingleTypeManager::<ChangesetInfo>::new(manager)),
        DerivableType::GitCommits => Arc::new(SingleTypeManager::<MappedGitCommitId>::new(manager)),
        DerivableType::GitDeltaManifestsV2 => {
            Arc::new(SingleTypeManager::<RootGitDeltaManifestV2Id>::new(manager))
        }
        DerivableType::GitDeltaManifestsV3 => {
            Arc::new(SingleTypeManager::<RootGitDeltaManifestV3Id>::new(manager))
        }
        DerivableType::InferredCopyFrom => {
            Arc::new(SingleTypeManager::<RootInferredCopyFromId>::new(manager))
        }
        DerivableType::BssmV3 => Arc::new(SingleTypeManager::<RootBssmV3DirectoryId>::new(manager)),
        DerivableType::TestManifests => {
            Arc::new(SingleTypeManager::<RootTestManifestDirectory>::new(manager))
        }
        DerivableType::TestShardedManifests => Arc::new(SingleTypeManager::<
            RootTestShardedManifestDirectory,
        >::new(manager)),
    }
}

#[async_trait]
impl BulkDerivation for DerivedDataManager {
    async fn derive_bulk_locally(
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
                cloned!(rederivation, visited, ctx);
                let csids = csids.to_vec();
                let manager = manager_for_type(self, *derived_data_type);
                async move {
                    mononoke::spawn_task(async move {
                        manager
                            .derive_heads_with_visited(
                                ctx,
                                csids,
                                override_batch_size,
                                rederivation,
                                visited,
                            )
                            .await
                    })
                    .await
                    .map_err(|err| {
                        SharedDerivationError::from(DerivationError::from(Error::from(err)))
                    })?
                }
            })
            .boxed()
            .buffer_unordered(10)
            .try_collect::<Vec<_>>()
            .await?;

        Ok(())
    }

    async fn derive_bulk(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_types: &[DerivableType],
        override_concurrency: Option<usize>,
    ) -> Result<(), SharedDerivationError> {
        stream::iter(
            derived_data_types
                .iter()
                .map(|ddt| manager_for_type(self, *ddt))
                .cartesian_product(csids),
        )
        .map(async |(manager, csid)| manager.derive(ctx, *csid, rederivation.clone()).await)
        .boxed()
        .buffer_unordered(override_concurrency.unwrap_or(10).max(1))
        .try_collect::<Vec<_>>()
        .await?;

        Ok(())
    }

    async fn derive_exactly_batch(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<(), DerivationError> {
        let manager = manager_for_type(self, derived_data_type);
        manager.derive_exactly_batch(ctx, csids, rederivation).await
    }

    async fn derive_exactly_underived_batch(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<(), DerivationError> {
        let manager = manager_for_type(self, derived_data_type);
        manager
            .derive_exactly_underived_batch(ctx, csids, rederivation)
            .await
    }

    async fn is_derived(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<bool, DerivationError> {
        let manager = manager_for_type(self, derived_data_type);
        manager.is_derived(ctx, csid, rederivation).await
    }

    async fn pending(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<Vec<ChangesetId>, DerivationError> {
        let manager = manager_for_type(self, derived_data_type);
        manager.pending(ctx, csids, rederivation).await
    }

    async fn fetch_derived_batch(
        &self,
        ctx: &CoreContext,
        csids: &[ChangesetId],
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<HashMap<ChangesetId, String>, DerivationError> {
        let manager = manager_for_type(self, derived_data_type);
        manager.fetch_derived_batch(ctx, csids, rederivation).await
    }

    async fn count_underived(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        limit: Option<u64>,
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<u64, DerivationError> {
        let manager = manager_for_type(self, derived_data_type);
        manager
            .count_underived(ctx, csid, limit, rederivation)
            .await
    }

    async fn derive_from_predecessor(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
        derived_data_type: DerivableType,
    ) -> Result<(), DerivationError> {
        let manager = manager_for_type(self, derived_data_type);
        manager
            .derive_from_predecessor(ctx, csid, rederivation)
            .await
    }
}
