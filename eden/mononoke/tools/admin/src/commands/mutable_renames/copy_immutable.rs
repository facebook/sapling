/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobstore::Loadable;
use changesets::ChangesetsRef;
use clap::Args;
use context::CoreContext;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use manifest::ManifestOps;
use mononoke_types::ChangesetId;
use mutable_renames::MutableRenameEntry;
use mutable_renames::MutableRenamesRef;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use std::collections::HashMap;
use std::collections::HashSet;
use unodes::RootUnodeManifestId;

use super::Repo;
use crate::commit_id::parse_commit_id;

#[derive(Args)]
pub struct CopyImmutableArgs {
    /// The CS to copy immutable history to mutable from
    ///
    /// This can be any commit id type.  Specify 'scheme=id' to disambiguate
    /// commit identity scheme (e.g. 'hg=HASH', 'globalrev=REV').
    commit_id: String,
}

pub async fn copy_immutable(ctx: &CoreContext, repo: &Repo, args: CopyImmutableArgs) -> Result<()> {
    let dst_cs_id = parse_commit_id(ctx, repo, &args.commit_id).await?;
    copy_immutable_impl(ctx, repo, dst_cs_id).await
}

pub async fn copy_immutable_impl(
    ctx: &CoreContext,
    repo: &Repo,
    dst_cs_id: ChangesetId,
) -> Result<()> {
    let bonsai_cs = dst_cs_id.load(ctx, repo.repo_blobstore()).await?;

    let changes_with_copies: Vec<_> = bonsai_cs
        .file_changes()
        .filter_map(|(dst_path, change)| {
            change
                .copy_from()
                .map(|(src_path, src_cs_id)| (dst_path.clone(), src_path.clone(), *src_cs_id))
        })
        .collect();

    let src_cs_ids: HashSet<ChangesetId> = changes_with_copies
        .iter()
        .map(|(_, _, cs_id)| *cs_id)
        .collect();
    let src_unode_manifest_ids: HashMap<_, RootUnodeManifestId> =
        stream::iter(src_cs_ids.into_iter().map(|cs_id| async move {
            let src_unode_manifest_id = repo
                .repo_derived_data()
                .derive::<RootUnodeManifestId>(ctx, cs_id)
                .await?;
            Ok::<_, Error>((cs_id, src_unode_manifest_id))
        }))
        .buffer_unordered(100)
        .try_collect()
        .await?;

    let entries = stream::iter(changes_with_copies.into_iter().map({
        let src_unode_manifest_ids = &src_unode_manifest_ids;
        move |(dst_path, src_path, src_cs_id)| async move {
            if let Some(src_unode_manifest_id) = src_unode_manifest_ids.get(&src_cs_id) {
                let src_entry = src_unode_manifest_id
                    .manifest_unode_id()
                    .find_entry(
                        ctx.clone(),
                        repo.repo_blobstore_arc(),
                        Some(src_path.clone()),
                    )
                    .await?
                    .with_context(|| {
                        format!(
                            "Cannot load source manifest entry, does `{}` exist in `{}`?",
                            src_path, src_cs_id,
                        )
                    })?;

                println!(
                    "Creating entry for `{}` copied to `{}`",
                    &src_path, &dst_path
                );
                MutableRenameEntry::new(
                    dst_cs_id,
                    Some(dst_path),
                    src_cs_id,
                    Some(src_path),
                    src_entry,
                )
            } else {
                bail!(
                    "Could not find unode manifest for path {} in cs {}!",
                    src_path,
                    src_cs_id
                );
            }
        }
    }))
    .buffer_unordered(100)
    .try_collect()
    .await?;

    repo.mutable_renames()
        .add_or_overwrite_renames(ctx, repo.changesets(), entries)
        .await?;
    Ok(())
}
