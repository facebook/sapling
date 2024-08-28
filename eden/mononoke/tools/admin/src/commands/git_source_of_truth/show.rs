/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use git_source_of_truth::GitSourceOfTruthConfigRef;
use git_source_of_truth::Staleness;
use repo_identity::RepoIdentityRef;

use super::Repo;

#[derive(Args)]
pub struct ShowArgs {}

pub async fn show(ctx: &CoreContext, repo: &Repo, _args: ShowArgs) -> Result<()> {
    let repo_id = repo.repo_identity().id();
    let maybe_entry = repo
        .git_source_of_truth_config()
        .get_by_repo_id(ctx, repo_id, Staleness::MostRecent)
        .await?;

    if let Some(entry) = maybe_entry {
        println!("{:?}", entry);
    } else {
        println!(
            "No git source of truth config entry found for repo {}",
            repo.repo_identity().name()
        );
    }

    Ok(())
}
