/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use clap::Parser;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;

use crate::commit_id::parse_commit_id;
use crate::commit_id::print_commit_id;
use crate::commit_id::IdentityScheme;

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

#[facet::container]
pub struct Repo {
    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let repo: Repo = app
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
