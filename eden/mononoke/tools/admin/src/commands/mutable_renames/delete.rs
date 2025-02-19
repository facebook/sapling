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
use mononoke_types::path::MPath;
use mutable_renames::MutableRenamesRef;

use super::Repo;

#[derive(Args)]
pub struct DeleteArgs {
    /// Commit ID to delete renames from
    ///
    /// This can be any commit id type.  Specify 'scheme=id' to disambiguate
    /// commit identity scheme (e.g. 'hg=HASH', 'globalrev=REV').
    commit_id: String,

    #[clap(long)]
    /// Optional path you wish to delete
    /// When specified, only the rename for the specific (commit, path) will be deleted
    /// Otherwise, all renames for the commit will be deleted
    path: Option<String>,

    #[clap(long)]
    /// Do not actually delete, only print the renames that would be deleted
    dry_run: bool,
}

pub async fn delete(ctx: &CoreContext, repo: &Repo, delete_args: DeleteArgs) -> Result<()> {
    let target_commit = parse_commit_id(ctx, repo, &delete_args.commit_id).await?;
    let mpath = match delete_args.path {
        Some(path) => Some(MPath::new(&path)?),
        None => None,
    };

    let renames = match mpath {
        Some(mpath) => repo
            .mutable_renames()
            .get_rename_uncached(ctx, target_commit, mpath)
            .await?
            .into_iter()
            .collect(),
        None => {
            repo.mutable_renames()
                .list_renames_by_dst_cs_uncached(ctx, target_commit)
                .await?
        }
    };

    if renames.is_empty() {
        println!("No mutable renames to delete");
        return Ok(());
    }

    println!(
        "The following {} mutable renames will be deleted:",
        renames.len()
    );
    for entry in &renames {
        println!(
            "\tDestination path {:?}, destination bonsai CS {}, source path {:?}, source bonsai CS {}, source unode {:?}",
            entry.dst_path(),
            target_commit,
            entry.src_path(),
            entry.src_cs_id(),
            entry.src_unode()
        );
    }

    if delete_args.dry_run {
        println!("Remove --dry-run to execute the deletion");
    } else {
        let (num_deleted_renames, num_deleted_paths) =
            repo.mutable_renames().delete_renames(ctx, renames).await?;
        println!(
            "Deleted {} mutable renames, {} paths",
            num_deleted_renames, num_deleted_paths
        );
    }

    Ok(())
}
