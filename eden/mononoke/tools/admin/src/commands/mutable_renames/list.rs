/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use commit_id::parse_commit_id;
use context::CoreContext;
use mutable_renames::MutableRenamesRef;

use super::Repo;

#[derive(Args)]
pub struct ListArgs {
    /// Commit ID to fetch renames from
    ///
    /// This can be any commit id type.  Specify 'scheme=id' to disambiguate
    /// commit identity scheme (e.g. 'hg=HASH', 'globalrev=REV').
    commit_id: String,
}

pub async fn list(ctx: &CoreContext, repo: &Repo, list_args: ListArgs) -> Result<()> {
    let target_commit = parse_commit_id(ctx, repo, &list_args.commit_id).await?;

    let entries = repo
        .mutable_renames()
        .list_renames_by_dst_cs_uncached(ctx, target_commit)
        .await?;

    if entries.is_empty() {
        println!("No mutable renames associated with this commit");
    } else {
        for entry in entries {
            println!(
                "Destination path {:?}, destination bonsai CS {}, source path {:?}, source bonsai CS {}, source unode {:?}",
                entry.dst_path(),
                target_commit,
                entry.src_path(),
                entry.src_cs_id(),
                entry.src_unode()
            );
        }
    }
    Ok(())
}
