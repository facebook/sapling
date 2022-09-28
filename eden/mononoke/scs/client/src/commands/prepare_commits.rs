/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use source_control::types as thrift;

use crate::args::commit_id::resolve_commit_ids;
use crate::args::commit_id::CommitIdsArgs;
use crate::args::derived_data_type::DerivedDataTypeArgs;
use crate::args::repo::RepoArgs;
use crate::ScscApp;

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
    let params = thrift::RepoPrepareCommitsParams {
        commits: resolve_commit_ids(
            &app.connection,
            &repo,
            &args.commit_ids.clone().into_commit_ids(),
        )
        .await?,
        derived_data_type: args.derived_data_type.clone().into_derived_data_type(),
        ..Default::default()
    };
    app.connection.repo_prepare_commits(&repo, &params).await?;
    Ok(())
}
