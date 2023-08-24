/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod partial_commit_graph;

use std::collections::HashMap;

use mononoke_api::ChangesetContext;
use mononoke_api::MononokeError;
use mononoke_api::RepoContext;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;

pub use crate::partial_commit_graph::build_partial_commit_graph_for_export;
use crate::partial_commit_graph::ChangesetParents;

/// Given a list of changesets, their parents and a list of paths, create
/// copies in a target mononoke repository containing only changes that
/// were made on the given paths.
pub async fn rewrite_partial_changesets(
    _source_repo_ctx: RepoContext,
    _changesets: Vec<ChangesetContext>,
    _changeset_parents: &ChangesetParents,
    // Repo that will hold the partial changesets that will be exported to git
    _target_repo_ctx: RepoContext,
    _export_paths: Vec<MPath>,
) -> Result<(), MononokeError> {
    todo!();
}

/// Given a changeset and a set of paths being exported, build the
/// BonsaiChangeset containing only the changes to those paths.
async fn _create_bonsai_for_new_repo(
    _source_repo_ctx: RepoContext,
    _export_paths: Vec<MPath>,
    _changeset_parents: &ChangesetParents,
    _remapped_parents: &mut HashMap<ChangesetId, ChangesetId>,
    _changeset_ctx: ChangesetContext,
) -> Result<BonsaiChangeset, MononokeError> {
    todo!();
}
