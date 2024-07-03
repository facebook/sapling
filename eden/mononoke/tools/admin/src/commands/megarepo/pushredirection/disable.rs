/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use clap::ArgAction;
use clap::Args;
use context::CoreContext;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_identity::RepoIdentity;

#[derive(Args)]
pub(super) struct DisableArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[arg(short, long, default_value_t = false, action = ArgAction::Set)]
    dry_run: bool,
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,
}

pub(super) async fn disable(_ctx: &CoreContext, app: MononokeApp, args: DisableArgs) -> Result<()> {
    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    Ok(())
}
