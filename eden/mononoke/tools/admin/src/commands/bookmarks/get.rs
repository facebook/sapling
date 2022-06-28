/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use bookmarks::BookmarkName;
use bookmarks::BookmarksRef;
use clap::Args;
use context::CoreContext;

use super::Repo;
use crate::commit_id::print_commit_id;
use crate::commit_id::IdentityScheme;

#[derive(Args)]
pub struct BookmarksGetArgs {
    /// Name of the bookmark to get
    name: BookmarkName,

    /// Commit identity schemes to display
    #[clap(long, short='S', arg_enum, default_values = &["bonsai"], use_value_delimiter = true)]
    schemes: Vec<IdentityScheme>,
}

pub async fn get(ctx: &CoreContext, repo: &Repo, get_args: BookmarksGetArgs) -> Result<()> {
    let bookmark_value = repo
        .bookmarks()
        .get(ctx.clone(), &get_args.name)
        .await
        .with_context(|| format!("Failed to resolve bookmark '{}'", get_args.name))?;

    match bookmark_value {
        None => println!("(not set)"),
        Some(cs_id) => print_commit_id(ctx, repo, &get_args.schemes, cs_id).await?,
    }

    Ok(())
}
