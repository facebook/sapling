/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_tag_mapping::BonsaiTagMapping;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use filestore::FilestoreConfig;
use metaconfig_types::RepoConfig;
use repo_blobstore::RepoBlobstore;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;

#[facet::container]
#[derive(Clone)]
pub struct HookTestRepo {
    #[facet]
    pub repo_identity: RepoIdentity,

    #[facet]
    pub repo_config: RepoConfig,

    #[facet]
    pub repo_blobstore: RepoBlobstore,

    #[facet]
    pub filestore_config: FilestoreConfig,

    #[facet]
    pub commit_graph: CommitGraph,

    #[facet]
    pub commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    pub bookmarks: dyn Bookmarks,

    #[facet]
    pub repo_derived_data: RepoDerivedData,

    #[facet]
    pub bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    pub bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    pub bonsai_tag_mapping: dyn BonsaiTagMapping,

    #[facet]
    pub repo_cross_repo: RepoCrossRepo,
}
