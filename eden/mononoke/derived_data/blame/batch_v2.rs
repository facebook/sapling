/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{anyhow, Error, Result};
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use derived_data::batch::{split_bonsais_in_linear_stacks, FileConflicts};
use derived_data_manager::DerivationContext;
use futures::stream::{FuturesOrdered, TryStreamExt};
use lock_ext::LockExt;
use mononoke_types::{BonsaiChangeset, ChangesetId};
use slog::debug;
use unodes::RootUnodeManifestId;

use crate::derive_v2::derive_blame_v2;
use crate::RootBlameV2;

pub async fn derive_blame_v2_in_batch(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsais: Vec<BonsaiChangeset>,
) -> Result<HashMap<ChangesetId, RootBlameV2>, Error> {
    let batch_len = bonsais.len();
    // We must split on any change as blame data must use the parent file.
    let linear_stacks = split_bonsais_in_linear_stacks(&bonsais, FileConflicts::AnyChange.into())?;
    let bonsais = Mutex::new(
        bonsais
            .into_iter()
            .map(|bcs| (bcs.get_changeset_id(), bcs))
            .collect::<HashMap<_, _>>(),
    );
    borrowed!(bonsais);

    let mut res = HashMap::new();
    for linear_stack in linear_stacks {
        if let Some(item) = linear_stack.file_changes.first() {
            debug!(
                ctx.logger(),
                "derive blame batch at {} (stack of {} from batch of {})",
                item.cs_id.to_hex(),
                linear_stack.file_changes.len(),
                batch_len,
            );
        }

        let new_blames = linear_stack
            .file_changes
            .into_iter()
            .map(|item| {
                // Clone owning copied to pass into the spawned future.
                cloned!(ctx, derivation_ctx);
                async move {
                    let csid = item.cs_id;
                    let bonsai = bonsais
                        .with(|bonsais| bonsais.remove(&csid))
                        .ok_or_else(|| anyhow!("changeset {} should be in bonsai batch", csid))?;
                    let derivation_fut = async move {
                        let root_manifest = derivation_ctx
                            .derive_dependency::<RootUnodeManifestId>(&ctx, csid)
                            .await?;
                        derive_blame_v2(&ctx, &derivation_ctx, bonsai, root_manifest).await?;
                        Ok::<_, Error>(root_manifest)
                    };
                    let derivation_handle = tokio::spawn(derivation_fut);
                    let root_manifest = derivation_handle.await??;
                    let derived = RootBlameV2 {
                        csid,
                        root_manifest,
                    };
                    Ok::<_, Error>((csid, derived))
                }
            })
            .collect::<FuturesOrdered<_>>()
            .try_collect::<Vec<_>>()
            .await?;

        res.extend(new_blames);
    }

    Ok(res)
}
