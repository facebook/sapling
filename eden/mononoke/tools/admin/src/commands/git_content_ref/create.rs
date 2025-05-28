/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Context;
use anyhow::Result;
use clap::Args;
use git_ref_content_mapping::GitRefContentMappingEntry;
use mononoke_types::hash::GitSha1;

use super::Repo;

#[derive(Args)]
pub struct CreateContentRefArgs {
    /// The name of the content ref
    #[clap(long)]
    ref_name: String,
    /// The git hash that the content ref points to
    #[clap(long)]
    git_hash: String,
    /// Whether the git hash is a tree (true) or a blob (false)
    #[clap(long)]
    is_tree: bool,
}

pub async fn create(repo: &Repo, create_args: CreateContentRefArgs) -> Result<()> {
    // Check if the content ref being added already exists
    let git_ref_content_mapping = repo.git_ref_content_mapping.clone();
    if let Some(content_ref_entry) = git_ref_content_mapping
        .get_entry_by_ref_name(create_args.ref_name.clone())
        .await?
    {
        anyhow::bail!(
            "The content ref {} already exists and it points to {} (is_tree: {})",
            content_ref_entry.ref_name,
            content_ref_entry.git_hash,
            content_ref_entry.is_tree
        );
    }

    // If the content ref doesn't exist, then create it
    let git_hash = GitSha1::from_str(&create_args.git_hash)
        .context("Error in parsing git hash from provided input")?;

    let success_msg = format!(
        "Content ref {} pointing to {} (is_tree: {}) has been added",
        create_args.ref_name, git_hash, create_args.is_tree
    );
    let entry = GitRefContentMappingEntry::new(create_args.ref_name, git_hash, create_args.is_tree);

    git_ref_content_mapping
        .add_or_update_mappings(vec![entry])
        .await?;
    println!("{}", success_msg);
    Ok(())
}
