/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::BookmarkUpdateLog;
use bookmarks::Bookmarks;
use bookmarks_cache::BookmarksCache;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use filenodes::Filenodes;
use filestore::FilestoreConfig;
use git_source_of_truth::GitSourceOfTruthConfig;
use hook_manager::HookManager;
use mercurial_mutation::HgMutationStore;
use metaconfig_types::RepoConfig;
use mutable_counters::MutableCounters;
use phases::Phases;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use repo_blobstore::RepoBlobstore;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use repo_lock::RepoLock;
use repo_permission_checker::RepoPermissionChecker;
use sql_query_config::SqlQueryConfig;
use streaming_clone::StreamingClone;

#[facet::container]
#[derive(Clone)]
pub struct RepoClientRepo(
    dyn BonsaiHgMapping,
    dyn BonsaiGitMapping,
    dyn BonsaiGlobalrevMapping,
    dyn PushrebaseMutationMapping,
    RepoCrossRepo,
    RepoBookmarkAttrs,
    dyn Bookmarks,
    dyn BookmarkUpdateLog,
    FilestoreConfig,
    dyn MutableCounters,
    dyn Phases,
    RepoBlobstore,
    RepoConfig,
    RepoDerivedData,
    RepoIdentity,
    CommitGraph,
    dyn CommitGraphWriter,
    dyn Filenodes,
    SqlQueryConfig,
    StreamingClone,
    dyn BookmarksCache,
    dyn HgMutationStore,
    dyn GitSourceOfTruthConfig,
    HookManager,
    dyn RepoLock,
    dyn RepoPermissionChecker,
);
