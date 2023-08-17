/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod create;
mod delete;
mod get;
mod update;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use git_symbolic_refs::GitSymbolicRefs;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_identity::RepoIdentity;

use self::create::CreateSymrefArgs;
use self::delete::DeleteSymrefArgs;
use self::get::GetSymrefArgs;
use self::update::UpdateSymrefArgs;

/// Perform git symbolic ref related operations.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: GitSymrefSubcommand,
}

#[facet::container]
pub struct Repo {
    #[facet]
    git_symbolic_refs: dyn GitSymbolicRefs,
    #[facet]
    repo_identity: RepoIdentity,
}

#[derive(Subcommand)]
pub enum GitSymrefSubcommand {
    /// Create Git Symref
    Create(CreateSymrefArgs),
    /// Update Git Symref
    Update(UpdateSymrefArgs),
    /// Get Git Symref
    Get(GetSymrefArgs),
    /// Delete Git Symref
    Delete(DeleteSymrefArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;
    match args.subcommand {
        GitSymrefSubcommand::Create(create_args) => create::create(&repo, create_args).await?,
        GitSymrefSubcommand::Update(update_args) => update::update(&repo, update_args).await?,
        GitSymrefSubcommand::Get(get_args) => get::get(&repo, get_args).await?,
        GitSymrefSubcommand::Delete(delete_args) => delete::delete(&repo, delete_args).await?,
    }
    Ok(())
}
