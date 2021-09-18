/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use derived_data::batch::{split_batch_in_linear_stacks, FileChangeAggregation, FileConflicts};
use derived_data::{BonsaiDerived, BonsaiDerivedMappingContainer};
use futures::stream::{FuturesOrdered, TryStreamExt};
use mononoke_types::ChangesetId;
use slog::debug;
use unodes::RootUnodeManifestId;

use crate::derive_v2::derive_blame_v2;
use crate::RootBlameV2;

pub async fn derive_blame_v2_in_batch(
    ctx: &CoreContext,
    repo: &BlobRepo,
    mapping: &BonsaiDerivedMappingContainer<RootBlameV2>,
    batch: Vec<ChangesetId>,
) -> Result<HashMap<ChangesetId, RootUnodeManifestId>, Error> {
    let batch_len = batch.len();
    // We must split on any change as blame data must use the parent file.
    let linear_stacks = split_batch_in_linear_stacks(
        ctx,
        repo.blobstore(),
        batch,
        FileConflicts::AnyChange,
        // For blame we don't care about file changes, so FileChangeAggregation can be anything
        FileChangeAggregation::Aggregate,
    )
    .await?;

    let mut res = HashMap::new();
    let options = mapping.options();
    for linear_stack in linear_stacks {
        if let Some((cs_id, _fc)) = linear_stack.file_changes.first() {
            debug!(
                ctx.logger(),
                "derive blame batch at {} (stack of {} from batch of {})",
                cs_id.to_hex(),
                linear_stack.file_changes.len(),
                batch_len,
            );
        }

        let new_blames = linear_stack
            .file_changes
            .into_iter()
            .map(|(cs_id, _fc)| {
                // Clone owning copied to pass into the spawned future.
                cloned!(ctx, repo, options);
                async move {
                    let derivation_fut = async move {
                        let bonsai = cs_id.load(&ctx, repo.blobstore()).await?;
                        let root_manifest = RootUnodeManifestId::derive(&ctx, &repo, cs_id).await?;
                        derive_blame_v2(&ctx, &repo, bonsai, root_manifest, &options).await?;
                        Ok::<_, Error>(root_manifest)
                    };
                    let derivation_handle = tokio::spawn(derivation_fut);
                    let root_manifest = derivation_handle.await??;
                    Ok::<_, Error>((cs_id, root_manifest))
                }
            })
            .collect::<FuturesOrdered<_>>()
            .try_collect::<Vec<_>>()
            .await?;

        res.extend(new_blames);
    }

    Ok(res)
}
