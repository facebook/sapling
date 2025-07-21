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
use git_ref_content_mapping::GitRefContentMapping;
use metaconfig_types::RepoConfig;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use repo_identity::RepoIdentity;

use self::create::CreateContentRefArgs;
use self::delete::DeleteContentRefArgs;
use self::get::GetContentRefArgs;
use self::update::UpdateContentRefArgs;

/// Perform git content ref related operations.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: GitContentRefSubcommand,
}

#[facet::container]
pub struct Repo {
    #[facet]
    git_ref_content_mapping: dyn GitRefContentMapping,
    #[facet]
    repo_identity: RepoIdentity,
    #[facet]
    repo_config: RepoConfig,
}

#[derive(Subcommand)]
pub enum GitContentRefSubcommand {
    /// Create Git Content Ref
    Create(CreateContentRefArgs),
    /// Update Git Content Ref
    Update(UpdateContentRefArgs),
    /// Get Git Content Ref
    Get(GetContentRefArgs),
    /// Delete Git Content Ref
    Delete(DeleteContentRefArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;
    let ctx = app.new_basic_context();
    match args.subcommand {
        GitContentRefSubcommand::Create(create_args) => {
            create::create(&repo, &ctx, create_args).await?
        }
        GitContentRefSubcommand::Update(update_args) => {
            update::update(&repo, &ctx, update_args).await?
        }
        GitContentRefSubcommand::Get(get_args) => get::get(&ctx, &repo, get_args).await?,
        GitContentRefSubcommand::Delete(delete_args) => {
            delete::delete(&repo, &ctx, delete_args).await?
        }
    }
    Ok(())
}
