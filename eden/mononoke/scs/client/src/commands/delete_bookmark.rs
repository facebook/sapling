/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use commit_id_types::OptionalCommitIdArgs;
use scs_client_raw::thrift;

use crate::ScscApp;
use crate::args::commit_id::resolve_commit_id;
use crate::args::pushvars::PushvarArgs;
use crate::args::repo::RepoArgs;
use crate::args::service_id::ServiceIdArgs;
use crate::errors::SelectionErrorExt;

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
    let conn = app.get_connection(Some(&repo.name))?;
    let old_target = match commit_id {
        Some(commit_id) => Some(resolve_commit_id(&conn, &repo, &commit_id).await?),
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
    conn.repo_delete_bookmark(&repo, &params)
        .await
        .map_err(|e| e.handle_selection_error(&repo))?;
    Ok(())
}
