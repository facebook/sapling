/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Result;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use fsnodes::RootFsnodeId;
use futures::future::try_join_all;
use futures::stream::TryStreamExt;
use manifest::ManifestOps;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use mononoke_types::inferred_copy_from::InferredCopyFrom;
use mononoke_types::inferred_copy_from::InferredCopyFromEntry;

// It's possible to have multiple source files that match,
// pick the one with the smallest path
fn pick_source_from_candidates(
    candidates: &[(ChangesetId, MPath)],
) -> Option<&(ChangesetId, MPath)> {
    candidates.iter().min_by_key(|(_, mpath)| mpath.clone())
}

async fn get_content_to_paths_from_changeset(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    cs_id: ChangesetId,
    paths: Vec<NonRootMPath>,
) -> Result<HashMap<ContentId, Vec<(ChangesetId, MPath)>>> {
    let mut content_to_paths = HashMap::new();

    let entries = derivation_ctx
        .fetch_dependency::<RootFsnodeId>(ctx, cs_id)
        .await?
        .fsnode_id()
        .find_entries(ctx.clone(), derivation_ctx.blobstore().clone(), paths)
        .try_collect::<Vec<_>>()
        .await?;

    for (path, entry) in entries {
        if let Some(content_id) = entry.into_leaf().map(|f| f.content_id().clone()) {
            content_to_paths
                .entry(content_id)
                .or_insert(vec![])
                .push((cs_id, path));
        }
    }
    Ok(content_to_paths)
}

// Find exact renames by comparing the content of deleted vs new/changed files
// in the current changeset. If they have the same content, the path pair is
// a rename.
async fn find_exact_renames(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
) -> Result<Vec<(MPath, InferredCopyFromEntry)>> {
    let mut content_to_paths = HashMap::new();
    for (path, file_change) in bonsai.simplified_file_changes() {
        if let Some(fc) = file_change {
            content_to_paths
                .entry(fc.content_id())
                .or_insert(vec![])
                .push(path.clone());
        }
    }

    let deleted_paths = bonsai
        .simplified_file_changes()
        .filter_map(|(path, fc)| {
            if fc.is_none() {
                Some(path.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let content_to_deleted_paths = try_join_all(bonsai.parents().map(|parent_cs_id| {
        cloned!(deleted_paths);
        async move {
            get_content_to_paths_from_changeset(ctx, derivation_ctx, parent_cs_id, deleted_paths)
                .await
                .with_context(|| {
                    format!(
                        "Failed to get content for deleted paths from parent {:?}",
                        parent_cs_id
                    )
                })
        }
    }))
    .await?
    .into_iter()
    .flatten()
    .collect::<HashMap<_, _>>();

    let mut renames = vec![];
    for (content_id, paths) in content_to_paths {
        if let Some(deleted_paths) = content_to_deleted_paths.get(&content_id) {
            let from = pick_source_from_candidates(deleted_paths).unwrap();
            for path in paths {
                renames.push((
                    MPath::from(path),
                    InferredCopyFromEntry {
                        from_csid: from.0,
                        from_path: from.1.clone(),
                    },
                ));
            }
        }
    }
    Ok(renames)
}

pub(crate) async fn derive_impl(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
) -> Result<Option<InferredCopyFrom>> {
    // TODO: add more cases
    // Ref: https://github.com/git/git/blob/master/diffcore-rename.c
    let entries = find_exact_renames(ctx, derivation_ctx, bonsai).await?;

    if entries.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            InferredCopyFrom::from_subentries(ctx, derivation_ctx.blobstore(), entries).await?,
        ))
    }
}
