/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use acl_regions::AclRegions;
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
use ephemeral_blobstore::RepoEphemeralStore;
use filenodes::Filenodes;
use filestore::FilestoreConfig;
use git_push_redirect::GitPushRedirectConfig;
use git_symbolic_refs::GitSymbolicRefs;
use mercurial_mutation::HgMutationStore;
use metaconfig_types::RepoConfig;
use mutable_counters::MutableCounters;
use mutable_renames::MutableRenames;
use phases::Phases;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use repo_blobstore::RepoBlobstore;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use repo_lock::RepoLock;
use repo_permission_checker::RepoPermissionChecker;
use repo_sparse_profiles::RepoSparseProfiles;
use sql_query_config::SqlQueryConfig;
use streaming_clone::StreamingClone;

// Eventually everything inside Repo should really be here
// The fields of BlobRepo that are not used in e.g. LFS server should also be moved here
// Each binary will then be able to only build what they use of the "repo attributes".
#[facet::container]
#[derive(Clone)]
pub struct InnerRepo {
    #[facet]
    pub filestore_config: FilestoreConfig,

    #[facet]
    pub repo_blobstore: RepoBlobstore,

    #[facet]
    pub repo_bookmark_attrs: RepoBookmarkAttrs,

    #[facet]
    pub repo_derived_data: RepoDerivedData,

    #[facet]
    pub repo_identity: RepoIdentity,

    #[facet]
    pub bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    pub bonsai_tag_mapping: dyn BonsaiTagMapping,

    #[facet]
    pub bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    pub bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    pub bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    pub bookmark_update_log: dyn BookmarkUpdateLog,

    #[facet]
    pub bookmarks: dyn Bookmarks,

    #[facet]
    pub filenodes: dyn Filenodes,

    #[facet]
    pub phases: dyn Phases,

    #[facet]
    pub pushrebase_mutation_mapping: dyn PushrebaseMutationMapping,

    #[facet]
    pub hg_mutation_store: dyn HgMutationStore,

    #[facet]
    pub mutable_counters: dyn MutableCounters,

    #[facet]
    pub repo_permission_checker: dyn RepoPermissionChecker,

    #[facet]
    pub repo_lock: dyn RepoLock,

    #[facet]
    pub commit_graph: CommitGraph,

    #[facet]
    pub commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    pub git_symbolic_refs: dyn GitSymbolicRefs,

    #[facet]
    pub commit_cloud: CommitCloud,

    #[facet]
    pub repo_config: RepoConfig,

    #[facet]
    pub ephemeral_store: RepoEphemeralStore,

    #[facet]
    pub mutable_renames: MutableRenames,

    #[facet]
    pub repo_cross_repo: RepoCrossRepo,

    #[facet]
    pub acl_regions: dyn AclRegions,

    #[facet]
    pub sparse_profiles: RepoSparseProfiles,

    #[facet]
    pub streaming_clone: StreamingClone,

    #[facet]
    pub sql_query_config: SqlQueryConfig,

    #[facet]
    pub git_push_redirect_config: dyn GitPushRedirectConfig,
}
