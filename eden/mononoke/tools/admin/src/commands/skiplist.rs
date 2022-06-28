/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod build;
mod read;

use anyhow::format_err;
use anyhow::Result;
use bookmarks::Bookmarks;
use build::SkiplistBuildArgs;
use changeset_fetcher::ChangesetFetcher;
use changesets::Changesets;
use clap::Parser;
use clap::Subcommand;
use metaconfig_types::RepoConfig;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use phases::Phases;
use read::SkiplistReadArgs;
use repo_blobstore::RepoBlobstore;

/// Build or read skiplist index for the repository
#[derive(Parser)]
pub struct CommandArgs {
    /// Blobstore key where to store the built skiplist
    #[clap(long, short = 'k')]
    blobstore_key: Option<String>,

    /// The repository name or ID
    #[clap(flatten)]
    repo: RepoArgs,

    /// The subcommand for skiplist index
    #[clap(subcommand)]
    subcommand: SkiplistSubcommand,
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    changeset_fetcher: dyn ChangesetFetcher,

    #[facet]
    changesets: dyn Changesets,

    #[facet]
    phases: dyn Phases,
}

#[derive(Subcommand)]
pub enum SkiplistSubcommand {
    /// Build the skiplist index and store it in blobstore
    Build(SkiplistBuildArgs),
    /// Read and display stored skiplist index
    Read(SkiplistReadArgs),
}

fn get_blobstore_key(key_arg: Option<String>, config: RepoConfig) -> Result<String> {
    match key_arg {
        Some(key_val) => Ok(key_val),
        None => match config.skiplist_index_blobstore_key {
            None => Err(format_err!(
                "no blobstore key provided as argument or in repository config"
            )),
            Some(key_val) => Ok(key_val),
        },
    }
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();
    let repo_arg = args.repo.id_or_name()?;
    let (_, repo_config) = app.repo_config(repo_arg)?;
    let logger = &app.logger();
    let key = get_blobstore_key(args.blobstore_key, repo_config)?;
    let repo: Repo = app.open_repo(&args.repo).await?;

    match args.subcommand {
        SkiplistSubcommand::Build(build_args) => {
            build::build_skiplist(&ctx, &repo, logger, key, build_args).await?
        }
        SkiplistSubcommand::Read(read_args) => {
            read::read_skiplist(&ctx, &repo, logger, key, read_args).await?
        }
    }
    Ok(())
}
