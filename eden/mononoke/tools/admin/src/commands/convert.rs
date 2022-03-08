/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Result};
use clap::Parser;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;

use crate::commit_id::{parse_commit_id, print_commit_id, IdentityScheme};
use crate::repo::AdminRepo;

/// Convert commit identity between identity schemes
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    /// Identity scheme to convert from
    #[clap(long, short = 'f', arg_enum, value_name = "SCHEME")]
    from: Option<IdentityScheme>,

    /// Identity scheme to
    #[clap(
        long,
        short = 't',
        arg_enum,
        value_name = "SCHEME",
        required = true,
        use_value_delimiter = true
    )]
    to: Vec<IdentityScheme>,

    /// Source commit id
    id: String,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let repo: AdminRepo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    let cs_id = match args.from {
        Some(scheme) => scheme.parse_commit_id(&ctx, &repo, &args.id).await?,
        None => parse_commit_id(&ctx, &repo, &args.id).await?,
    };

    print_commit_id(&ctx, &repo, &args.to, cs_id).await?;

    Ok(())
}
