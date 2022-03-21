/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod create_key_list;
mod list;

use anyhow::Result;
use clap::{Parser, Subcommand};
use mononoke_app::MononokeApp;

use create_key_list::{RedactionCreateKeyListArgs, RedactionCreateKeyListFromIdsArgs};
use list::RedactionListArgs;

/// Manage repository bookmarks
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(subcommand)]
    subcommand: RedactionSubcommand,
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
