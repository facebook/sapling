/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use filenodes::Filenodes;
use phases::Phases;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use sql_commit_graph_storage::CommitGraphBulkFetcher;

#[facet::container]
#[derive(Clone)]
pub struct Repo {
    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_bulk_fetcher: CommitGraphBulkFetcher,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    phases: dyn Phases,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    filenodes: dyn Filenodes,
}
