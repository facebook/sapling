/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use changesets::ChangesetsRef;
use clap::Args;
use context::CoreContext;
use futures::TryStreamExt;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathOrPrefix;
use mononoke_types::mpath_element_iter;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::MPath;
use mononoke_types::ManifestUnodeId;
use mutable_renames::MutableRenameEntry;
use mutable_renames::MutableRenamesRef;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use std::collections::HashMap;
use unodes::RootUnodeManifestId;

use super::Repo;
use crate::commands::mutable_renames::copy_immutable;
use crate::commit_id::parse_commit_id;

#[derive(Args)]
pub struct AddArgs {
    #[clap(long)]
    /// The source CS of the mutable history
    ///
    /// This can be any commit id type.  Specify 'scheme=id' to disambiguate
    /// commit identity scheme (e.g. 'hg=HASH', 'globalrev=REV').
    src_commit_id: String,

    #[clap(long)]
    /// The source path (copy from)
    src_path: String,

    #[clap(long)]
    /// The destination CS of the mutable history
    ///
    /// This can be any commit id type.  Specify 'scheme=id' to disambiguate
    /// commit identity scheme (e.g. 'hg=HASH', 'globalrev=REV').
    dst_commit_id: String,

    #[clap(long)]
    /// The destination path (copied to)
    dst_path: String,

    #[clap(long)]
    /// If set, do not recurse and create mutable history entries
    /// to cover all sub-paths
    no_recurse: bool,

    #[clap(long)]
    /// Do not actually do the database change; just list the work that would be done
    dry_run: bool,
}

pub async fn add(ctx: &CoreContext, repo: &Repo, add_args: AddArgs) -> Result<()> {
    let src_cs_id = parse_commit_id(ctx, repo, &add_args.src_commit_id).await?;
    let src_path = MPath::new_opt(&add_args.src_path)?;
    let dst_cs_id = parse_commit_id(ctx, repo, &add_args.dst_commit_id).await?;
    let dst_path = MPath::new_opt(&add_args.dst_path)?;

    // If we don't have mutable renames on a commit already, copy over the
    // immutable renames before adding new ones
    if !repo
        .mutable_renames()
        .has_rename_uncached(ctx, dst_cs_id)
        .await?
    {
        copy_immutable::copy_immutable_impl(ctx, repo, dst_cs_id).await?;
    }

    let src_root_unode_id = repo
        .repo_derived_data()
        .derive::<RootUnodeManifestId>(ctx, src_cs_id)
        .await?;
    let dst_root_unode_id = repo
        .repo_derived_data()
        .derive::<RootUnodeManifestId>(ctx, dst_cs_id)
        .await?;

    let src_entry = src_root_unode_id
        .manifest_unode_id()
        .find_entry(ctx.clone(), repo.repo_blobstore_arc(), src_path.clone())
        .await?
        .with_context(|| {
            format!(
                "Cannot load source manifest entry, does `{}` exist?",
                MPath::display_opt(src_path.as_ref())
            )
        })?;

    let entries = match (add_args.no_recurse, &src_entry) {
        (true, _) | (_, Entry::Leaf(_)) => {
            let dst_entry = dst_root_unode_id
                .manifest_unode_id()
                .find_entry(ctx.clone(), repo.repo_blobstore_arc(), dst_path.clone())
                .await?
                .with_context(|| {
                    format!(
                        "Cannot load destination manifest entry, does `{}` exist?",
                        MPath::display_opt(dst_path.as_ref())
                    )
                })?;
            Ok(vec![create_mutable_rename(
                src_cs_id, src_path, src_entry, dst_cs_id, dst_path, dst_entry,
            )??])
        }
        (false, Entry::Tree(src_unode_manifest)) => {
            let mut src_entries: HashMap<_, _> = src_unode_manifest
                .list_all_entries(ctx.clone(), repo.repo_blobstore_arc())
                .try_collect()
                .await?;
            let mut dst_entries: HashMap<_, _> = dst_root_unode_id
                .manifest_unode_id()
                .find_entries(
                    ctx.clone(),
                    repo.repo_blobstore_arc(),
                    [PathOrPrefix::Prefix(dst_path.clone())],
                )
                .try_collect()
                .await?;

            let first = match (src_entries.remove(&None), dst_entries.remove(&dst_path)) {
                (Some(src_entry), Some(dst_entry)) => Some(create_mutable_rename(
                    src_cs_id,
                    src_path.clone(),
                    src_entry,
                    dst_cs_id,
                    dst_path.clone(),
                    dst_entry,
                )?),
                (None, _) => {
                    bail!("Source checked earlier, but has vanished! The repo may be corrupt!")
                }
                (_, None) => bail!(
                    "Cannot load destination manifest entry, does `{}` exist?",
                    MPath::display_opt(dst_path.as_ref())
                ),
            };

            first.into_iter().chain(src_entries
                .into_iter()
                .filter_map(|(src_entry_path, src_entry)| {
                    let dst_entry_path =
                        MPath::join_opt(dst_path.as_ref(), mpath_element_iter(&src_entry_path));
                    let src_entry_path =
                        MPath::join_opt(src_path.as_ref(), mpath_element_iter(&src_entry_path));
                    let dst_entry = dst_entries.remove(&dst_entry_path);

                    if let Some(dst_entry) = dst_entry {
                        create_mutable_rename(src_cs_id, src_entry_path, src_entry, dst_cs_id, dst_entry_path, dst_entry).map_err(|e| {eprintln!("{}",e); e}).ok()
                    } else {
                        eprintln!(
                            "Source path `{}` has no matching destination `{}` - no entry created.",
                            MPath::display_opt(src_entry_path.as_ref()),
                            MPath::display_opt(dst_entry_path.as_ref())
                        );
                        None
                    }
                }))
                .collect()
        }
    }?;

    if !add_args.dry_run {
        repo.mutable_renames()
            .add_or_overwrite_renames(ctx, repo.changesets(), entries)
            .await?;
    }
    Ok(())
}

fn create_mutable_rename(
    src_cs_id: ChangesetId,
    src_path: Option<MPath>,
    src_entry: Entry<ManifestUnodeId, FileUnodeId>,
    dst_cs_id: ChangesetId,
    dst_path: Option<MPath>,
    dst_entry: Entry<ManifestUnodeId, FileUnodeId>,
) -> Result<Result<MutableRenameEntry>> {
    if dst_entry.is_tree() != src_entry.is_tree() {
        bail!(
            "Source `{}` is a {}, destination `{}` is a {} - no entry created.",
            MPath::display_opt(src_path.as_ref()),
            entry_to_type(&src_entry),
            MPath::display_opt(dst_path.as_ref()),
            entry_to_type(&dst_entry)
        );
    }

    println!(
        "Creating entry for source {} `{}` to destination {} `{}`",
        entry_to_type(&src_entry),
        MPath::display_opt(src_path.as_ref()),
        entry_to_type(&dst_entry),
        MPath::display_opt(dst_path.as_ref())
    );

    Ok(MutableRenameEntry::new(
        dst_cs_id, dst_path, src_cs_id, src_path, src_entry,
    ))
}

fn entry_to_type<T, L>(entry: &Entry<T, L>) -> &'static str {
    if entry.is_tree() { "directory" } else { "file" }
}
