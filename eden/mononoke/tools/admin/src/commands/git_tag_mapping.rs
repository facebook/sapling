/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod reconcile;

use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_tag_mapping::BonsaiTagMapping;
use bookmarks::Bookmarks;
use clap::Parser;
use clap::Subcommand;
use metaconfig_types::RepoConfig;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use repo_blobstore::RepoBlobstore;
use repo_identity::RepoIdentity;

use self::reconcile::ReconcileArgs;

/// Perform bonsai_tag_mapping (git tag mapping) operations.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: GitTagMappingSubcommand,
}

#[facet::container]
pub struct Repo {
    #[facet]
    bonsai_tag_mapping: dyn BonsaiTagMapping,
    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,
    #[facet]
    bookmarks: dyn Bookmarks,
    #[facet]
    repo_blobstore: RepoBlobstore,
    #[facet]
    repo_identity: RepoIdentity,
    #[facet]
    repo_config: RepoConfig,
}

#[derive(Subcommand)]
pub enum GitTagMappingSubcommand {
    /// Find (and optionally recover) annotated tags whose tags/<tag> bookmark has
    /// diverged from their tag object, by moving the bookmark back to the tag's
    /// target. One-time cleanup for S687348.
    Reconcile(ReconcileArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;
    let ctx = app.new_basic_context();
    match args.subcommand {
        GitTagMappingSubcommand::Reconcile(reconcile_args) => {
            reconcile::reconcile(&repo, &ctx, reconcile_args).await?
        }
    }
    Ok(())
}
