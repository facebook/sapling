/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use commit_id_types::CommitIdsArgs;
use scs_client_raw::thrift;

use crate::ScscApp;
use crate::args::commit_id::resolve_commit_ids;
use crate::args::derived_data_type::DerivedDataTypeArgs;
use crate::args::repo::RepoArgs;
use crate::errors::SelectionErrorExt;

#[derive(clap::Parser)]
/// Prepare a commit by deriving the required derived data type
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,
    #[clap(flatten)]
    commit_ids: CommitIdsArgs,
    #[clap(flatten)]
    derived_data_type: DerivedDataTypeArgs,
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo.clone().into_repo_specifier();
    let conn = app.get_connection(Some(&repo.name))?;
    let params = thrift::RepoPrepareCommitsParams {
        commits: resolve_commit_ids(&conn, &repo, &args.commit_ids.clone().into_commit_ids())
            .await?,
        derived_data_type: args.derived_data_type.clone().into_derived_data_type(),
        ..Default::default()
    };
    conn.repo_prepare_commits(&repo, &params)
        .await
        .map_err(|e| e.handle_selection_error(&repo))?;
    Ok(())
}
