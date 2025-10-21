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
use git_ref_content_mapping::GitRefContentMapping;
use git_symbolic_refs::GitSymbolicRefs;
use metaconfig_types::RepoConfig;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use restricted_paths::RestrictedPaths;

#[facet::container]
#[derive(Clone)]
pub(crate) struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_config: RepoConfig,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    bonsai_tag_mapping: dyn BonsaiTagMapping,

    #[facet]
    git_ref_content_mapping: dyn GitRefContentMapping,

    #[facet]
    git_symbolic_refs: dyn GitSymbolicRefs,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    restricted_paths: RestrictedPaths,
}
