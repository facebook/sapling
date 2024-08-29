/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use git_source_of_truth::GitSourceOfTruth;
use git_source_of_truth::GitSourceOfTruthConfigRef;
use git_source_of_truth::RepositoryName;
use repo_identity::RepoIdentityRef;

use super::Repo;

#[derive(Args)]
pub struct SetArgs {
    /// Source of truth to set for this repo. [possible values: metagit, mononoke, locked]
    source_of_truth: GitSourceOfTruth,
}

pub async fn set(ctx: &CoreContext, repo: &Repo, args: SetArgs) -> Result<()> {
    let repo_id = repo.repo_identity().id();
    let repo_name = repo.repo_identity().name();

    repo.git_source_of_truth_config()
        .set(
            ctx,
            repo_id,
            RepositoryName(repo_name.to_string()),
            args.source_of_truth,
        )
        .await?;

    Ok(())
}
