/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bonsai_tag_mapping::BonsaiTagMapping;
use bookmarks::BookmarkUpdateLog;
use bookmarks::Bookmarks;
use commit_cloud::CommitCloud;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use ephemeral_blobstore::Bubble;
use filenodes::Filenodes;
use filestore::FilestoreConfig;
use git_symbolic_refs::GitSymbolicRefs;
use mercurial_mutation::HgMutationStore;
use metaconfig_types::RepoConfig;
use mutable_counters::MutableCounters;
use phases::Phases;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use repo_lock::RepoLock;
use repo_permission_checker::RepoPermissionChecker;
use sql_commit_graph_storage::CommitGraphBulkFetcher;

// NOTE: this structure and its fields are public to enable `DangerousOverride` functionality
#[facet::container]
#[derive(Clone)]
pub struct BlobRepoInner {
    #[facet]
    pub repo_identity: RepoIdentity,

    #[init(repo_identity.name().to_string())]
    pub reponame: String,

    #[facet]
    pub config: RepoConfig,

    #[facet]
    pub repo_blobstore: RepoBlobstore,

    #[facet]
    pub commit_graph: CommitGraph,

    #[facet]
    pub commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    pub bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    pub bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    pub bonsai_tag_mapping: dyn BonsaiTagMapping,

    #[facet]
    pub git_symbolic_refs: dyn GitSymbolicRefs,

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

    #[facet]
    pub commit_cloud: CommitCloud,

    #[facet]
    pub commit_graph_bulk_fetcher: CommitGraphBulkFetcher,
}

#[facet::container]
#[derive(Clone)]
pub struct BlobRepo {
    #[delegate(
        RepoIdentity,
        RepoBlobstore,
        CommitGraph,
        dyn CommitGraphWriter,
        dyn BonsaiHgMapping,
        dyn BonsaiGitMapping,
        dyn BonsaiTagMapping,
        dyn BonsaiGlobalrevMapping,
        dyn BonsaiSvnrevMapping,
        dyn PushrebaseMutationMapping,
        dyn Bookmarks,
        dyn BookmarkUpdateLog,
        dyn Phases,
        FilestoreConfig,
        dyn Filenodes,
        dyn GitSymbolicRefs,
        dyn HgMutationStore,
        RepoDerivedData,
        RepoConfig,
        dyn MutableCounters,
        dyn RepoPermissionChecker,
        dyn RepoLock,
        RepoBookmarkAttrs,
        CommitCloud,
        CommitGraphBulkFetcher,
    )]
    inner: Arc<BlobRepoInner>,
}

impl BlobRepo {
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
        let blobstore = bubble.wrap_repo_blobstore(self.repo_blobstore().clone());
        let commit_graph = Arc::new(bubble.repo_commit_graph(self));
        let repo_derived_data = self.inner.repo_derived_data.for_bubble(bubble);
        let mut inner = (*self.inner).clone();
        inner.repo_derived_data = Arc::new(repo_derived_data);
        inner.repo_blobstore = Arc::new(blobstore);
        inner.commit_graph = commit_graph;
        Self {
            inner: Arc::new(inner),
        }
    }
}
