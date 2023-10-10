/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod create;

use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_tag_mapping::BonsaiTagMapping;
use bookmarks::Bookmarks;
use clap::Parser;
use clap::Subcommand;
use commit_graph::CommitGraph;
use git_symbolic_refs::GitSymbolicRefs;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;

use self::create::CreateBundleArgs;

/// Perform git related operations.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: GitBundleSubcommand,
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,
    #[facet]
    commit_graph: CommitGraph,
    #[facet]
    bookmarks: dyn Bookmarks,
    #[facet]
    repo_derived_data: RepoDerivedData,
    #[facet]
    repo_blobstore: RepoBlobstore,
    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,
    #[facet]
    bonsai_tag_mapping: dyn BonsaiTagMapping,
    #[facet]
    git_symbolic_refs: dyn GitSymbolicRefs,
}

#[derive(Subcommand)]
pub enum GitBundleSubcommand {
    /// Create Git bundle
    Create(CreateBundleArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;
    match args.subcommand {
        GitBundleSubcommand::Create(create_args) => create::create(&ctx, create_args, repo).await?,
    }
    Ok(())
}
