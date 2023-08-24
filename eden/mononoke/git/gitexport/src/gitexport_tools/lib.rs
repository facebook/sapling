/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod partial_commit_graph;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Error;
use blobstore::Loadable;
use borrowed::borrowed;
use cloned::cloned;
use commit_transformation::rewrite_commit;
use commit_transformation::upload_commits;
use commit_transformation::MultiMover;
use futures::stream::TryStreamExt;
use futures::stream::{self};
use mononoke_api::BookmarkKey;
use mononoke_api::ChangesetContext;
use mononoke_api::CoreContext;
use mononoke_api::MononokeError;
use mononoke_api::RepoContext;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use repo_blobstore::RepoBlobstoreArc;
use slog::debug;
use slog::error;
use slog::info;

pub use crate::partial_commit_graph::build_partial_commit_graph_for_export;
use crate::partial_commit_graph::ChangesetParents;

/// Given a list of changesets, their parents and a list of paths, create
/// copies in a target mononoke repository containing only changes that
/// were made on the given paths.
pub async fn rewrite_partial_changesets(
    source_repo_ctx: RepoContext,
    changesets: Vec<ChangesetContext>,
    changeset_parents: &ChangesetParents,
    // Repo that will hold the partial changesets that will be exported to git
    target_repo_ctx: RepoContext,
    export_paths: Vec<MPath>,
) -> Result<(), MononokeError> {
    let ctx: &CoreContext = source_repo_ctx.ctx();

    let logger = ctx.logger();

    debug!(logger, "export_paths: {:#?}", &export_paths);
    debug!(logger, "changeset_parents: {:#?}", &changeset_parents);

    let logger_clone = logger.clone();

    let multi_mover: MultiMover = Arc::new(move |source_path: &MPath| {
        let should_export = export_paths.iter().any(|p| p.is_prefix_of(source_path));

        if !should_export {
            debug!(
                logger_clone,
                "Path {:#?} will NOT be exported.", &source_path
            );
            return Ok(vec![]);
        }

        debug!(logger_clone, "Path {:#?} will be exported.", &source_path);
        Ok(vec![source_path.clone()])
    });

    let cs_results: Vec<Result<ChangesetContext, MononokeError>> =
        changesets.into_iter().map(Ok).collect::<Vec<_>>();

    let (new_bonsai_changesets, _) = stream::iter(cs_results)
        .try_fold(
            (Vec::new(), HashMap::new()),
            |(mut new_bonsai_changesets, remapped_parents), changeset| {
                borrowed!(source_repo_ctx);
                cloned!(multi_mover);
                async move {
                    let (new_bcs, remapped_parents) = create_bonsai_for_new_repo(
                        source_repo_ctx,
                        multi_mover,
                        changeset_parents,
                        remapped_parents,
                        changeset,
                    )
                    .await?;
                    new_bonsai_changesets.push(new_bcs);
                    Ok((new_bonsai_changesets, remapped_parents))
                }
            },
        )
        .await?;

    debug!(
        logger,
        "new_bonsai_changesets: {:#?}", &new_bonsai_changesets
    );

    let head_cs_id = new_bonsai_changesets
        .last()
        .ok_or(Error::msg("No changesets were moved"))?
        .get_changeset_id();

    upload_commits(
        source_repo_ctx.ctx(),
        new_bonsai_changesets,
        source_repo_ctx.repo(),
        target_repo_ctx.repo(),
    )
    .await?;

    // Set master bookmark to point to the latest changeset
    if let Err(err) = target_repo_ctx
        .create_bookmark(&BookmarkKey::from_str("master")?, head_cs_id, None)
        .await
    {
        // TODO(T161902005): stop failing silently on bookmark creation
        error!(logger, "Failed to create master bookmark: {:?}", err);
    }

    info!(logger, "Finished copying all changesets!");
    Ok(())
}

/// Given a changeset and a set of paths being exported, build the
/// BonsaiChangeset containing only the changes to those paths.
async fn create_bonsai_for_new_repo(
    source_repo_ctx: &RepoContext,
    multi_mover: MultiMover,
    changeset_parents: &ChangesetParents,
    mut remapped_parents: HashMap<ChangesetId, ChangesetId>,
    changeset_ctx: ChangesetContext,
) -> Result<(BonsaiChangeset, HashMap<ChangesetId, ChangesetId>), MononokeError> {
    let logger = changeset_ctx.repo().ctx().logger();
    debug!(
        logger,
        "Rewriting changeset: {:#?} | {:#?}",
        &changeset_ctx.id(),
        &changeset_ctx.message().await?
    );

    let blobstore = source_repo_ctx.blob_repo().repo_blobstore_arc();
    let bcs: BonsaiChangeset = changeset_ctx
        .id()
        .load(source_repo_ctx.ctx(), &blobstore)
        .await
        .map_err(MononokeError::from)?;

    // The BonsaiChangeset was built with the full graph, so the parents are
    // not necessarily part of the provided changesets. So we first need to
    // update the parents to reference the parents from the partial graph.
    //
    // These values will be used to read the `remapped_parents` map in
    // `rewrite_commit` and get the correct changeset id for the parents in
    // the new bonsai changeset.
    let orig_parent_ids: Vec<ChangesetId> = changeset_parents
        .get(&bcs.get_changeset_id())
        .cloned()
        .unwrap_or_default();
    let mut mut_bcs = bcs.into_mut();
    mut_bcs.parents = orig_parent_ids.clone();

    let rewritten_bcs_mut = rewrite_commit(
        source_repo_ctx.ctx(),
        mut_bcs,
        &remapped_parents,
        multi_mover,
        source_repo_ctx.repo(),
        None,
        Default::default(),
    )
    .await?
    // This shouldn't happen because every changeset provided is modifying
    // at least one of the exported files.
    .ok_or(Error::msg(
        "Commit wasn't rewritten because it had no signficant changes",
    ))?;

    let rewritten_bcs = rewritten_bcs_mut.freeze()?;

    // Update the remapped_parents map so that children of this commit use
    // its new id when moved.
    remapped_parents.insert(changeset_ctx.id(), rewritten_bcs.get_changeset_id());

    Ok((rewritten_bcs, remapped_parents))
}
