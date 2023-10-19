/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod create;

use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_tag_mapping::BonsaiTagMapping;
use bookmarks::Bookmarks;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use commit_graph::CommitGraph;
use git_symbolic_refs::GitSymbolicRefs;
use mononoke_app::MononokeApp;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;

use self::create::FromPathArgs;
use self::create::FromRepoArgs;

/// Perform git related operations.
#[derive(Parser)]
pub struct CommandArgs {
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

#[derive(Args)]
/// Arguments for creating a Git bundle
pub struct CreateBundleArgs {
    #[clap(subcommand)]
    commands: CreateBundleCommands,
}

#[derive(Subcommand)]
enum CreateBundleCommands {
    FromPath(FromPathArgs),
    FromRepo(FromRepoArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    match args.subcommand {
        GitBundleSubcommand::Create(create_args) => match create_args.commands {
            CreateBundleCommands::FromPath(create_args) => {
                create::create_from_path(create_args).await
            }
            CreateBundleCommands::FromRepo(create_args) => {
                create::create_from_mononoke_repo(&ctx, &app, create_args).await
            }
        },
    }
}
