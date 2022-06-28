/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::future;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use manifest::find_intersection_of_diffs;
use mononoke_types::blame::store_blame;
use mononoke_types::blame::Blame;
use mononoke_types::blame::BlameId;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::MPath;
use std::collections::HashMap;
use std::sync::Arc;
use unodes::find_unode_renames_incorrect_for_blame_v1;
use unodes::RootUnodeManifestId;

use crate::fetch::fetch_content_for_blame_with_limit;
use crate::fetch::FetchOutcome;
use crate::DEFAULT_BLAME_FILESIZE_LIMIT;

pub(crate) async fn derive_blame_v1(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: BonsaiChangeset,
    root_manifest: RootUnodeManifestId,
) -> Result<(), Error> {
    let csid = bonsai.get_changeset_id();
    let parents_mf = async {
        let parents = derivation_ctx
            .fetch_parents::<RootUnodeManifestId>(ctx, &bonsai)
            .await?;
        Ok(parents
            .into_iter()
            .map(|parent| parent.manifest_unode_id().clone())
            .collect::<Vec<_>>())
    };

    let (parents_mf, renames) = future::try_join(
        parents_mf,
        find_unode_renames_incorrect_for_blame_v1(ctx, derivation_ctx, &bonsai),
    )
    .await?;

    let filesize_limit = derivation_ctx
        .config()
        .blame_filesize_limit
        .unwrap_or(DEFAULT_BLAME_FILESIZE_LIMIT);
    let renames = Arc::new(renames);
    let blobstore = derivation_ctx.blobstore();
    find_intersection_of_diffs(
        ctx.clone(),
        blobstore.clone(),
        root_manifest.manifest_unode_id().clone(),
        parents_mf,
    )
    .map_ok(|(path, entry)| Some((path?, entry.into_leaf()?)))
    .try_filter_map(future::ok)
    .map(move |v| {
        cloned!(ctx, blobstore, renames);
        async move {
            let (path, file) = v?;
            Result::<_>::Ok(
                tokio::spawn(async move {
                    create_blame_v1(&ctx, &blobstore, renames, csid, path, file, filesize_limit)
                        .await
                })
                .await??,
            )
        }
    })
    .buffered(256)
    .try_for_each(|_| future::ok(()))
    .await?;

    Ok(())
}

async fn create_blame_v1(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    renames: Arc<HashMap<MPath, FileUnodeId>>,
    csid: ChangesetId,
    path: MPath,
    file_unode_id: FileUnodeId,
    filesize_limit: u64,
) -> Result<BlameId, Error> {
    let file_unode = file_unode_id.load(ctx, blobstore).await?;

    let parents_content_and_blame: Vec<_> = file_unode
        .parents()
        .iter()
        .cloned()
        .chain(renames.get(&path).cloned())
        .map(|file_unode_id| {
            future::try_join(
                fetch_content_for_blame_with_limit(ctx, blobstore, file_unode_id, filesize_limit),
                async move { BlameId::from(file_unode_id).load(ctx, blobstore).await }.err_into(),
            )
        })
        .collect();

    let (content, parents_content) = future::try_join(
        fetch_content_for_blame_with_limit(ctx, blobstore, file_unode_id, filesize_limit),
        future::try_join_all(parents_content_and_blame),
    )
    .await?;

    let blame_maybe_rejected = match content {
        FetchOutcome::Rejected(rejected) => rejected.into(),
        FetchOutcome::Fetched(content) => {
            let parents_content = parents_content
                .into_iter()
                .filter_map(|(content, blame_maybe_rejected)| {
                    Some((
                        content.into_bytes().ok()?,
                        blame_maybe_rejected.into_blame().ok()?,
                    ))
                })
                .collect();
            Blame::from_parents(csid, content, path, parents_content)?.into()
        }
    };

    store_blame(ctx, blobstore, file_unode_id, blame_maybe_rejected).await
}
