/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::format_err;
use anyhow::Error;
use bonsai_git_mapping::ArcBonsaiGitMapping;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::ArcBonsaiGlobalrevMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::ArcBonsaiHgMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::ArcBonsaiSvnrevMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::ArcBookmarkUpdateLog;
use bookmarks::ArcBookmarks;
use bookmarks::BookmarkUpdateLog;
use bookmarks::Bookmarks;
use cacheblob::LeaseOps;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcher;
use changeset_fetcher::SimpleChangesetFetcher;
use changesets::Changesets;
use changesets::ChangesetsRef;
use context::CoreContext;
use ephemeral_blobstore::Bubble;
use filenodes::ArcFilenodes;
use filenodes::Filenodes;
use filestore::FilestoreConfig;
use mercurial_mutation::ArcHgMutationStore;
use mercurial_mutation::HgMutationStore;
use metaconfig_types::DerivedDataConfig;
use metaconfig_types::DerivedDataTypesConfig;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use mononoke_types::RepositoryId;
use mutable_counters::MutableCounters;
use phases::Phases;
use pushrebase_mutation_mapping::ArcPushrebaseMutationMapping;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use repo_lock::ArcRepoLock;
use repo_lock::RepoLock;
use repo_permission_checker::ArcRepoPermissionChecker;
use repo_permission_checker::RepoPermissionChecker;
use stats::prelude::*;

define_stats! {
    prefix = "mononoke.blobrepo";
    get_changeset_parents_by_bonsai: timeseries(Rate, Sum),
    get_generation_number: timeseries(Rate, Sum),
}

// NOTE: this structure and its fields are public to enable `DangerousOverride` functionality
#[facet::container]
#[derive(Clone)]
pub struct BlobRepoInner {
    #[facet]
    pub repo_identity: RepoIdentity,

    #[init(repo_identity.id())]
    pub repoid: RepositoryId,

    #[init(repo_identity.name().to_string())]
    pub reponame: String,

    #[facet]
    pub repo_blobstore: RepoBlobstore,

    #[facet]
    pub changesets: dyn Changesets,

    #[facet]
    pub changeset_fetcher: dyn ChangesetFetcher,

    #[facet]
    pub bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    pub bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    pub bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    pub bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    pub pushrebase_mutation_mapping: dyn PushrebaseMutationMapping,

    #[facet]
    pub bookmarks: dyn Bookmarks,

    #[facet]
    pub bookmark_update_log: dyn BookmarkUpdateLog,

    #[facet]
    pub phases: dyn Phases,

    #[facet]
    pub filestore_config: FilestoreConfig,

    #[facet]
    pub filenodes: dyn Filenodes,

    #[facet]
    pub hg_mutation_store: dyn HgMutationStore,

    #[facet]
    pub repo_derived_data: RepoDerivedData,

    #[facet]
    pub mutable_counters: dyn MutableCounters,

    #[facet]
    pub permission_checker: dyn RepoPermissionChecker,

    #[facet]
    pub repo_lock: dyn RepoLock,

    #[facet]
    pub repo_bookmark_attrs: RepoBookmarkAttrs,
}

#[facet::container]
#[derive(Clone)]
pub struct BlobRepo {
    #[delegate(
        RepoIdentity,
        RepoBlobstore,
        dyn Changesets,
        dyn ChangesetFetcher,
        dyn BonsaiHgMapping,
        dyn BonsaiGitMapping,
        dyn BonsaiGlobalrevMapping,
        dyn BonsaiSvnrevMapping,
        dyn PushrebaseMutationMapping,
        dyn Bookmarks,
        dyn BookmarkUpdateLog,
        dyn Phases,
        FilestoreConfig,
        dyn Filenodes,
        dyn HgMutationStore,
        RepoDerivedData,
        dyn MutableCounters,
        dyn RepoPermissionChecker,
        dyn RepoLock,
        RepoBookmarkAttrs,
    )]
    inner: Arc<BlobRepoInner>,
}

impl BlobRepo {
    #[inline]
    pub fn bookmarks(&self) -> &ArcBookmarks {
        &self.inner.bookmarks
    }

    #[inline]
    pub fn bookmark_update_log(&self) -> &ArcBookmarkUpdateLog {
        &self.inner.bookmark_update_log
    }

    #[inline]
    pub fn bonsai_hg_mapping(&self) -> &ArcBonsaiHgMapping {
        &self.inner.bonsai_hg_mapping
    }

    #[inline]
    pub fn changeset_fetcher(&self) -> &ArcChangesetFetcher {
        &self.inner.changeset_fetcher
    }

    #[inline]
    pub fn filenodes(&self) -> &ArcFilenodes {
        &self.inner.filenodes
    }

    #[inline]
    pub fn hg_mutation_store(&self) -> &ArcHgMutationStore {
        &self.inner.hg_mutation_store
    }

    pub async fn get_changeset_parents_by_bonsai(
        &self,
        ctx: CoreContext,
        changesetid: ChangesetId,
    ) -> Result<Vec<ChangesetId>, Error> {
        STATS::get_changeset_parents_by_bonsai.add_value(1);
        let changeset = self.changesets().get(ctx, changesetid).await?;
        let parents = changeset
            .ok_or_else(|| format_err!("Commit {} does not exist in the repo", changesetid))?
            .parents;
        Ok(parents)
    }

    pub fn bonsai_git_mapping(&self) -> &ArcBonsaiGitMapping {
        &self.inner.bonsai_git_mapping
    }

    pub fn bonsai_globalrev_mapping(&self) -> &ArcBonsaiGlobalrevMapping {
        &self.inner.bonsai_globalrev_mapping
    }

    pub fn bonsai_svnrev_mapping(&self) -> &ArcBonsaiSvnrevMapping {
        &self.inner.bonsai_svnrev_mapping
    }

    pub fn pushrebase_mutation_mapping(&self) -> &ArcPushrebaseMutationMapping {
        &self.inner.pushrebase_mutation_mapping
    }

    // Returns the generation number of a changeset
    // note: it returns Option because changeset might not exist
    pub async fn get_generation_number(
        &self,
        ctx: CoreContext,
        cs: ChangesetId,
    ) -> Result<Option<Generation>, Error> {
        STATS::get_generation_number.add_value(1);
        let result = self.inner.changesets.get(ctx, cs).await?;
        Ok(result.map(|res| Generation::new(res.gen)))
    }

    pub fn get_changeset_fetcher(&self) -> ArcChangesetFetcher {
        self.changeset_fetcher().clone()
    }

    pub fn blobstore(&self) -> &RepoBlobstore {
        &self.inner.repo_blobstore
    }

    pub fn get_blobstore(&self) -> RepoBlobstore {
        self.inner.repo_blobstore.as_ref().clone()
    }

    pub fn filestore_config(&self) -> FilestoreConfig {
        *self.inner.filestore_config
    }

    pub fn get_repoid(&self) -> RepositoryId {
        self.inner.repoid
    }

    pub fn name(&self) -> &String {
        &self.inner.reponame
    }

    pub fn get_derived_data_config(&self) -> &DerivedDataConfig {
        self.inner.repo_derived_data.config()
    }

    pub fn get_active_derived_data_types_config(&self) -> &DerivedDataTypesConfig {
        self.inner.repo_derived_data.manager().config()
    }

    pub fn get_derived_data_types_config(&self, name: &str) -> Option<&DerivedDataTypesConfig> {
        self.inner.repo_derived_data.config().get_config(name)
    }

    pub fn get_derived_data_lease_ops(&self) -> Arc<dyn LeaseOps> {
        self.inner.repo_derived_data.lease().clone()
    }

    /// To be used by `DangerousOverride` only
    pub fn inner(&self) -> &Arc<BlobRepoInner> {
        &self.inner
    }

    /// To be used by `DagerouseOverride` only
    pub fn from_inner_dangerous(inner: BlobRepoInner) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }

    pub fn with_bubble(&self, bubble: Bubble) -> Self {
        let blobstore = bubble.wrap_repo_blobstore(self.get_blobstore());
        let changesets = Arc::new(bubble.changesets(self));
        let changeset_fetcher = SimpleChangesetFetcher::new(changesets.clone(), self.get_repoid());
        let new_manager = self
            .inner
            .repo_derived_data
            .manager()
            .clone()
            .for_bubble(bubble, self);
        let repo_derived_data = self.inner.repo_derived_data.with_manager(new_manager);
        let mut inner = (*self.inner).clone();
        inner.repo_derived_data = Arc::new(repo_derived_data);
        inner.changesets = changesets;
        inner.changeset_fetcher = Arc::new(changeset_fetcher);
        inner.repo_blobstore = Arc::new(blobstore);
        Self {
            inner: Arc::new(inner),
        }
    }

    #[inline]
    pub fn permission_checker(&self) -> ArcRepoPermissionChecker {
        self.inner.permission_checker.clone()
    }

    #[inline]
    pub fn repo_lock(&self) -> ArcRepoLock {
        self.inner.repo_lock.clone()
    }
}

/// Compatibility trait for conversion between a facet-style repo (that
/// happens to contain a BlobRepo) and the blob repo (for calling things that
/// still require blobrepo).
pub trait AsBlobRepo {
    fn as_blob_repo(&self) -> &BlobRepo;
}

impl AsBlobRepo for BlobRepo {
    fn as_blob_repo(&self) -> &BlobRepo {
        self
    }
}

/// This function uploads bonsai changests object to blobstore in parallel, and then does
/// sequential writes to changesets table. Parents of the changesets should already by saved
/// in the repository.
pub async fn save_bonsai_changesets(
    bonsai_changesets: Vec<BonsaiChangeset>,
    ctx: CoreContext,
    container: &(impl ChangesetsRef + RepoBlobstoreRef),
) -> Result<(), Error> {
    changesets_creation::save_changesets(&ctx, container, bonsai_changesets).await
}
