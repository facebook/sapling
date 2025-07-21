/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;

use super::Repo;

#[derive(Args)]
pub struct GetContentRefArgs {
    /// The name of the content ref
    #[clap(long)]
    ref_name: String,
}

pub async fn get(ctx: &CoreContext, repo: &Repo, get_args: GetContentRefArgs) -> Result<()> {
    match repo
        .git_ref_content_mapping
        .get_entry_by_ref_name(ctx, get_args.ref_name.clone())
        .await?
    {
        Some(content_ref_entry) => println!(
            "The content ref {} points to {} (is_tree: {})",
            content_ref_entry.ref_name, content_ref_entry.git_hash, content_ref_entry.is_tree
        ),
        None => println!("Content ref {} not found", get_args.ref_name),
    }
    Ok(())
}
