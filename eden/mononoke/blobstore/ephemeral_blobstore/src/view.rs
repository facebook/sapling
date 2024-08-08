/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use metaconfig_types::RepoConfig;
use repo_blobstore::RepoBlobstore;
use repo_identity::RepoIdentity;

#[facet::container]
pub struct EphemeralRepoView {
    #[facet]
    pub(crate) repo_blobstore: RepoBlobstore,

    #[facet]
    pub(crate) commit_graph: CommitGraph,

    #[facet]
    pub(crate) commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    pub(crate) bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    pub(crate) repo_identity: RepoIdentity,

    #[facet]
    pub(crate) repo_config: RepoConfig,
}
