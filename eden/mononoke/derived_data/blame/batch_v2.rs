/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use derived_data::batch::split_bonsais_in_linear_stacks;
use derived_data::batch::FileConflicts;
use derived_data::batch::SplitOptions;
use derived_data::batch::DEFAULT_STACK_FILE_CHANGES_LIMIT;
use derived_data_manager::DerivationContext;
use futures::stream::FuturesOrdered;
use futures::stream::TryStreamExt;
use lock_ext::LockExt;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
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
    // We must split on any change as blame data must use the parent file, and
    // must split on copy info as this also affects blame.
    let linear_stacks = split_bonsais_in_linear_stacks(
        &bonsais,
        SplitOptions {
            file_conflicts: FileConflicts::AnyChange,
            copy_info: true,
            file_changes_limit: DEFAULT_STACK_FILE_CHANGES_LIMIT,
        },
    )?;
    let bonsais = Mutex::new(
        bonsais
            .into_iter()
            .map(|bcs| (bcs.get_changeset_id(), bcs))
            .collect::<HashMap<_, _>>(),
    );
    borrowed!(bonsais);

    let mut res = HashMap::new();
    for linear_stack in linear_stacks {
        let stack_len = linear_stack.stack_items.len();
        if let Some(item) = linear_stack.stack_items.first() {
            debug!(
                ctx.logger(),
                "derive blame batch at {} (stack of {} from batch of {})",
                item.cs_id.to_hex(),
                stack_len,
                batch_len,
            );
        }

        let new_blames = linear_stack
            .stack_items
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
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
                        derive_blame_v2(&ctx, &derivation_ctx, bonsai, root_manifest)
                            .await
                            .with_context(|| {
                                format!(
                                    concat!(
                                        "failed to derive blame_v2 for {}, ",
                                        "index {} in stack of {} from batch of {}"
                                    ),
                                    csid, index, stack_len, batch_len
                                )
                            })?;
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
