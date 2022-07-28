/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use source_control::types as thrift;

use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::OptionalCommitIdArgs;
use crate::args::pushvars::PushvarArgs;
use crate::args::repo::RepoArgs;
use crate::args::service_id::ServiceIdArgs;
use crate::ScscApp;

#[derive(clap::Parser)]
/// Delete a bookmark
///
/// If a commit id is provided, the bookmark is only deleted if it currently
/// points at that commit.
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    service_id_args: ServiceIdArgs,
    #[clap(flatten)]
    pushvar_args: PushvarArgs,
    #[clap(flatten)]
    commit_id_args: OptionalCommitIdArgs,
    #[clap(long, short)]
    /// Name of the bookmark to delete
    name: String,
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.into_repo_specifier();
    let commit_id = args.commit_id_args.into_commit_id();
    let old_target = match commit_id {
        Some(commit_id) => Some(resolve_commit_id(&app.connection, &repo, &commit_id).await?),
        None => None,
    };
    let bookmark = args.name;
    let service_identity = args.service_id_args.service_id;
    let pushvars = args.pushvar_args.into_pushvars();

    let params = thrift::RepoDeleteBookmarkParams {
        bookmark,
        old_target,
        service_identity,
        pushvars,
        ..Default::default()
    };
    app.connection.repo_delete_bookmark(&repo, &params).await?;
    Ok(())
}
