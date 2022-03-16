/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use mutable_renames::MutableRenamesRef;

use crate::commit_id::parse_commit_id;
use crate::repo::AdminRepo;

#[derive(Args)]
pub struct CheckCommitArgs {
    /// Commit ID to check
    ///
    /// This can be any commit id type.  Specify 'scheme=id' to disambiguate
    /// commit identity scheme (e.g. 'hg=HASH', 'globalrev=REV').
    commit_id: String,

    #[clap(long)]
    /// Bypass the cache and go straight to MySQL
    bypass_cache: bool,
}

pub async fn check_commit(
    ctx: &CoreContext,
    repo: &AdminRepo,
    check_commit_args: CheckCommitArgs,
) -> Result<()> {
    let target_commit = parse_commit_id(ctx, repo, &check_commit_args.commit_id).await?;

    let has_rename = if check_commit_args.bypass_cache {
        repo.mutable_renames()
            .has_rename_uncached(ctx, target_commit)
            .await?
    } else {
        repo.mutable_renames()
            .has_rename(ctx, target_commit)
            .await?
    };

    if has_rename {
        println!("Commit has mutable renames associated with some paths");
    } else {
        println!("No mutable renames associated with this commit");
    }
    Ok(())
}
