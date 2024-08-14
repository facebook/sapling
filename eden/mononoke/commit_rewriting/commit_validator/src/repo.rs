/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bookmarks::BookmarkUpdateLog;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use mutable_counters::MutableCounters;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use sql_query_config::SqlQueryConfig;

#[facet::container]
#[derive(Clone)]
pub struct Repo {
    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    mutable_counters: dyn MutableCounters,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    bookmark_update_log: dyn BookmarkUpdateLog,

    #[facet]
    sql_query_config: SqlQueryConfig,
}
