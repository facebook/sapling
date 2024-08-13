/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_tag_mapping::BonsaiTagMapping;
use bookmarks::BookmarkUpdateLog;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use filenodes::Filenodes;
use filestore::FilestoreConfig;
use metaconfig_types::RepoConfig;
use mononoke_types::RepositoryId;
use mutable_counters::MutableCounters;
use phases::Phases;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use pushredirect::PushRedirectionConfig;
use repo_blobstore::RepoBlobstore;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use sql_query_config::SqlQueryConfig;
use synced_commit_mapping::SyncedCommitMapping;

#[facet::container]
#[derive(Clone)]
pub struct Repo {
    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_bookmark_attrs: RepoBookmarkAttrs,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_tag_mapping: dyn BonsaiTagMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    bookmark_update_log: dyn BookmarkUpdateLog,

    #[facet]
    phases: dyn Phases,

    #[facet]
    pushrebase_mutation_mapping: dyn PushrebaseMutationMapping,

    #[facet]
    mutable_counters: dyn MutableCounters,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    filenodes: dyn Filenodes,

    #[facet]
    synced_commit_mapping: dyn SyncedCommitMapping,

    #[facet]
    repo_cross_repo: RepoCrossRepo,

    #[facet]
    config: RepoConfig,

    #[facet]
    push_redirection_config: dyn PushRedirectionConfig,

    #[facet]
    sql_query_config: SqlQueryConfig,
}

impl Repo {
    pub fn repo_id(&self) -> RepositoryId {
        self.repo_identity().id()
    }

    pub fn name(&self) -> &str {
        self.repo_identity().name()
    }
}
