/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use mononoke_types::MPath;
use mutable_renames::MutableRenamesRef;

use super::Repo;
use crate::commit_id::parse_commit_id;

#[derive(Args)]
pub struct GetArgs {
    /// Commit ID to fetch renames from
    ///
    /// This can be any commit id type.  Specify 'scheme=id' to disambiguate
    /// commit identity scheme (e.g. 'hg=HASH', 'globalrev=REV').
    commit_id: String,

    #[clap(long)]
    /// The path you wish to check
    path: String,

    #[clap(long)]
    /// Bypass the cache and go straight to MySQL
    bypass_cache: bool,
}

pub async fn get(ctx: &CoreContext, repo: &Repo, get_args: GetArgs) -> Result<()> {
    let target_commit = parse_commit_id(ctx, repo, &get_args.commit_id).await?;

    let mpath = MPath::new_opt(&get_args.path)?;

    let maybe_entry = if get_args.bypass_cache {
        repo.mutable_renames()
            .get_rename_uncached(ctx, target_commit, mpath)
            .await?
    } else {
        repo.mutable_renames()
            .get_rename(ctx, target_commit, mpath)
            .await?
    };

    match maybe_entry {
        None => println!("No mutable rename for that path and commit"),
        Some(entry) => {
            println!(
                "Source path `{}`, source bonsai CS {}, source unode {:?}",
                entry
                    .src_path()
                    .as_ref()
                    .map_or(String::new(), |p| p.to_string()),
                entry.src_cs_id(),
                entry.src_unode()
            );
        }
    }
    Ok(())
}
