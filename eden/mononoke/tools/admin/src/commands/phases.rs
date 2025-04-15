/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod add_public;
mod fetch;
mod list_public;

use add_public::AddPublicArgs;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::Bookmarks;
use clap::Parser;
use clap::Subcommand;
use fetch::FetchArgs;
use list_public::ListPublicArgs;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use phases::Phases;

/// Commands to work with phases
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: PhasesSubcommand,
}

#[derive(Subcommand)]
enum PhasesSubcommand {
    /// Fetch the phase of a commit
    Fetch(FetchArgs),
    /// List all public commits in the repo
    ListPublic(ListPublicArgs),
    /// Mark commits and their ancestors as public
    AddPublic(AddPublicArgs),
}

#[facet::container]
pub struct Repo {
    #[facet]
    phases: dyn Phases,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    bookmarks: dyn Bookmarks,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let repo: Repo = app.open_repo(&args.repo).await?;

    match args.subcommand {
        PhasesSubcommand::Fetch(args) => fetch::fetch(&ctx, &repo, args).await,
        PhasesSubcommand::ListPublic(args) => list_public::list_public(&ctx, &repo, args).await,
        PhasesSubcommand::AddPublic(args) => add_public::add_public(&ctx, &repo, args).await,
    }
}
