/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use commit_id_types::CommitIdArgs;
use scs_client_raw::thrift;

use crate::ScscApp;
use crate::args::commit_id::resolve_commit_id;
use crate::args::pushvars::PushvarArgs;
use crate::args::repo::RepoArgs;
use crate::args::service_id::ServiceIdArgs;
use crate::errors::SelectionErrorExt;

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
    let conn = app.get_connection(Some(&repo.name))?;
    let id = resolve_commit_id(&conn, &repo, &commit_id).await?;
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
    conn.repo_create_bookmark(&repo, &params)
        .await
        .map_err(|e| e.handle_selection_error(&repo))?;
    Ok(())
}
