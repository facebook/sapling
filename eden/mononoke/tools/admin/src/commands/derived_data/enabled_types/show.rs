/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use enabled_derived_data_types::EnabledDerivedDataTypesRef;
use enabled_derived_data_types::Staleness;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use repo_identity::RepoIdentityRef;

use super::super::Repo;

#[derive(Args)]
pub(super) struct ShowArgs {
    #[clap(flatten)]
    repo: RepoArgs,
}

pub(super) async fn show(ctx: &CoreContext, app: &MononokeApp, args: ShowArgs) -> Result<()> {
    let repo: Repo = app.open_repo(&args.repo).await?;
    let repo_id = repo.repo_identity().id();

    let mut types = repo
        .enabled_derived_data_types()
        .get_enabled_types(ctx, repo_id, Staleness::MostRecent)
        .await?;
    types.sort();

    if types.is_empty() {
        println!("No derived data types enabled for repo {repo_id}");
    } else {
        for ddt in types {
            println!("{}", ddt.name());
        }
    }

    Ok(())
}
