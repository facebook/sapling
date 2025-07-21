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
use context::CoreContext;
use git_ref_content_mapping::GitRefContentMappingEntry;
use mononoke_types::hash::GitSha1;
use repo_update_logger::GitContentRefInfo;
use repo_update_logger::log_git_content_ref;

use super::Repo;

#[derive(Args)]
pub struct UpdateContentRefArgs {
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

pub async fn update(
    repo: &Repo,
    ctx: &CoreContext,
    update_args: UpdateContentRefArgs,
) -> Result<()> {
    let git_hash = GitSha1::from_str(&update_args.git_hash)
        .context("Error in parsing git hash from provided input")?;

    let success_msg = format!(
        "Content ref {} pointing to {} (is_tree: {}) has been updated",
        update_args.ref_name, git_hash, update_args.is_tree
    );
    let entry =
        GitRefContentMappingEntry::new(update_args.ref_name.clone(), git_hash, update_args.is_tree);

    repo.git_ref_content_mapping
        .add_or_update_mappings(ctx, vec![entry])
        .await?;
    let info = GitContentRefInfo {
        repo_name: repo.repo_identity.name().to_string(),
        ref_name: update_args.ref_name,
        git_hash: update_args.git_hash,
        object_type: match update_args.is_tree {
            true => "tree".to_string(),
            false => "blob".to_string(),
        },
    };
    log_git_content_ref(ctx, &repo, &info).await;
    println!("{}", success_msg);
    Ok(())
}
