/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use source_control::types as thrift;

use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::CommitIdArgs;
use crate::args::pushvars::PushvarArgs;
use crate::args::repo::RepoArgs;
use crate::args::service_id::ServiceIdArgs;
use crate::ScscApp;

#[derive(clap::Parser)]
/// Create a bookmark
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
    #[clap(flatten)]
    service_id_args: ServiceIdArgs,
    #[clap(flatten)]
    pushvar_args: PushvarArgs,
    #[clap(long, short)]
    /// Name of the bookmark to create
    name: String,
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_id = args.commit_id_args.clone().into_commit_id();
    let id = resolve_commit_id(&app.connection, &repo, &commit_id).await?;
    let bookmark = args.name.clone();
    let service_identity = args.service_id_args.service_id.clone();
    let pushvars = args.pushvar_args.into_pushvars();

    let params = thrift::RepoCreateBookmarkParams {
        bookmark,
        target: id,
        service_identity,
        pushvars,
        ..Default::default()
    };
    app.connection.repo_create_bookmark(&repo, &params).await?;
    Ok(())
}
