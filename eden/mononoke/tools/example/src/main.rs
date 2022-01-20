/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Parser;
use fbinit::FacebookInit;
use mononoke_app::{MononokeApp, MononokeAppBuilder};
use mononoke_args::repo::RepoArgs;
use repo_identity::{RepoIdentity, RepoIdentityRef};

/// Display the repo identity of the chosen repo.
#[derive(Parser)]
struct ExampleArgs {
    #[clap(flatten)]
    repo: RepoArgs,
}

#[facet::container]
struct Repo {
    #[facet]
    repo_identity: RepoIdentity,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    MononokeAppBuilder::new(fb)
        .build::<ExampleArgs>()?
        .run(async_main)
}

async fn async_main(app: MononokeApp) -> Result<()> {
    let args: ExampleArgs = app.args()?;

    let repo: Repo = app.open_repo(&args.repo).await?;

    println!(
        "Repo Id: {} Name: {}",
        repo.repo_identity().id(),
        repo.repo_identity().name(),
    );

    Ok(())
}
