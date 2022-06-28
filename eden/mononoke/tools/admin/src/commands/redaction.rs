/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod create_key_list;
mod list;

use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::Bookmarks;
use clap::Parser;
use clap::Subcommand;
use metaconfig_types::RepoConfig;
use mononoke_app::MononokeApp;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;

use create_key_list::RedactionCreateKeyListArgs;
use create_key_list::RedactionCreateKeyListFromIdsArgs;
use list::RedactionListArgs;

/// Manage repository bookmarks
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(subcommand)]
    subcommand: RedactionSubcommand,
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_config: RepoConfig,

    #[facet]
    bookmarks: dyn Bookmarks,

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
}

#[derive(Subcommand)]
pub enum RedactionSubcommand {
    /// Create a key list using files in a changeset.
    CreateKeyList(RedactionCreateKeyListArgs),
    /// Create a key list using content ids.
    CreateKeyListFromIds(RedactionCreateKeyListFromIdsArgs),
    /// List the redacted files in a commit.
    List(RedactionListArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    match args.subcommand {
        RedactionSubcommand::CreateKeyList(create_args) => {
            create_key_list::create_key_list_from_commit_files(&ctx, &app, create_args).await?
        }
        RedactionSubcommand::CreateKeyListFromIds(create_args) => {
            create_key_list::create_key_list_from_blobstore_keys(&ctx, &app, create_args).await?
        }
        RedactionSubcommand::List(list_args) => list::list(&ctx, &app, list_args).await?,
    }

    Ok(())
}
