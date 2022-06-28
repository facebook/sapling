/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod add;
mod check_commit;
mod copy_immutable;
mod get;

use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use changesets::Changesets;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mutable_renames::MutableRenames;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;

use add::AddArgs;
use check_commit::CheckCommitArgs;
use copy_immutable::CopyImmutableArgs;
use get::GetArgs;

/// Fetch and update mutable renames information
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: MutableRenamesSubcommand,
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

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    mutable_renames: MutableRenames,

    #[facet]
    changesets: dyn Changesets,
}

#[derive(Subcommand)]
pub enum MutableRenamesSubcommand {
    /// Determine if a commit has mutable rename information attached
    CheckCommit(CheckCommitArgs),
    /// Get mutable rename information for a given commit, path pair
    Get(GetArgs),
    /// Add new mutable renames to your repo
    Add(AddArgs),
    /// Copy immutable renames to mutable renames
    CopyImmutable(CopyImmutableArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    match args.subcommand {
        MutableRenamesSubcommand::CheckCommit(args) => {
            check_commit::check_commit(&ctx, &repo, args).await?
        }
        MutableRenamesSubcommand::Get(args) => get::get(&ctx, &repo, args).await?,
        MutableRenamesSubcommand::Add(args) => add::add(&ctx, &repo, args).await?,
        MutableRenamesSubcommand::CopyImmutable(args) => {
            copy_immutable::copy_immutable(&ctx, &repo, args).await?
        }
    }

    Ok(())
}
