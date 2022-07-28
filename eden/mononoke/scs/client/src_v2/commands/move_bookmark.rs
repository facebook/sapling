/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Result;
use source_control::types as thrift;

use crate::args::commit_id::resolve_commit_ids;
use crate::args::commit_id::CommitIdsArgs;
use crate::args::pushvars::PushvarArgs;
use crate::args::repo::RepoArgs;
use crate::args::service_id::ServiceIdArgs;
use crate::ScscApp;

#[derive(clap::Parser)]
/// Move a bookmark
///
/// If two commits are provided, then move the bookmark from the first commit
/// to the second commit, failing if the bookmark didn't previously point at
/// the first commit.
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    commit_ids_args: CommitIdsArgs,
    #[clap(flatten)]
    service_id_args: ServiceIdArgs,
    #[clap(flatten)]
    pushvar_args: PushvarArgs,
    #[clap(long, short)]
    /// Name of the bookmark to move
    name: String,
    #[clap(long)]
    /// Allow non-fast-forward moves (if permitted for this bookmark)
    allow_non_fast_forward_move: bool,
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.into_repo_specifier();
    let commit_ids = args.commit_ids_args.into_commit_ids();
    if commit_ids.len() != 1 && commit_ids.len() != 2 {
        bail!("expected 1 or 2 commit_ids (got {})", commit_ids.len())
    }
    let ids = resolve_commit_ids(&app.connection, &repo, &commit_ids).await?;
    let bookmark = args.name;
    let service_identity = args.service_id_args.service_id;

    let (old_target, target) = match ids.as_slice() {
        [id] => (None, id.clone()),
        [old_id, new_id] => (Some(old_id.clone()), new_id.clone()),
        _ => bail!("expected 1 or 2 commit_ids (got {})", ids.len()),
    };
    let allow_non_fast_forward_move = args.allow_non_fast_forward_move;
    let pushvars = args.pushvar_args.into_pushvars();

    let params = thrift::RepoMoveBookmarkParams {
        bookmark,
        target,
        old_target,
        service_identity,
        allow_non_fast_forward_move,
        pushvars,
        ..Default::default()
    };
    app.connection.repo_move_bookmark(&repo, &params).await?;
    Ok(())
}
