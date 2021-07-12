/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Error, Result};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use futures::{future, StreamExt, TryFutureExt, TryStreamExt};
use manifest::find_intersection_of_diffs;
use mononoke_types::{
    blame::{store_blame, Blame, BlameId},
    BonsaiChangeset, ChangesetId, FileUnodeId, MPath,
};
use std::collections::HashMap;
use std::sync::Arc;
use unodes::{find_unode_renames_incorrect_for_blame_v1, RootUnodeManifestId};

use crate::fetch::{fetch_content_for_blame, FetchOutcome};
use crate::BlameDeriveOptions;

pub(crate) async fn derive_blame_v1(
    ctx: CoreContext,
    repo: BlobRepo,
    bonsai: BonsaiChangeset,
    root_manifest: RootUnodeManifestId,
    options: &BlameDeriveOptions,
) -> Result<(), Error> {
    let blame_options = *options;
    let csid = bonsai.get_changeset_id();
    let parents_manifest = bonsai
        .parents()
        .collect::<Vec<_>>() // iterator should be owned
        .into_iter()
        .map(|csid| {
            RootUnodeManifestId::derive(&ctx, &repo, csid)
                .map_ok(|root_id| root_id.manifest_unode_id().clone())
        });

    let (parents_mf, renames) = future::try_join(
        future::try_join_all(parents_manifest).err_into(),
        find_unode_renames_incorrect_for_blame_v1(ctx.clone(), repo.clone(), &bonsai),
    )
    .await?;

    let renames = Arc::new(renames);
    let blobstore = repo.get_blobstore().boxed();
    find_intersection_of_diffs(
        ctx.clone(),
        blobstore,
        root_manifest.manifest_unode_id().clone(),
        parents_mf,
    )
    .map_ok(|(path, entry)| Some((path?, entry.into_leaf()?)))
    .try_filter_map(future::ok)
    .map(move |v| {
        cloned!(ctx, repo, renames);
        async move {
            let (path, file) = v?;
            Result::<_>::Ok(
                tokio::spawn(async move {
                    create_blame_v1(&ctx, &repo, renames, csid, path, file, blame_options).await
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
    repo: &BlobRepo,
    renames: Arc<HashMap<MPath, FileUnodeId>>,
    csid: ChangesetId,
    path: MPath,
    file_unode_id: FileUnodeId,
    options: BlameDeriveOptions,
) -> Result<BlameId, Error> {
    let blobstore = repo.blobstore();

    let file_unode = file_unode_id.load(ctx, blobstore).await?;

    let parents_content_and_blame: Vec<_> = file_unode
        .parents()
        .iter()
        .cloned()
        .chain(renames.get(&path).cloned())
        .map(|file_unode_id| {
            future::try_join(
                fetch_content_for_blame(ctx, repo, file_unode_id, options),
                async move { BlameId::from(file_unode_id).load(ctx, blobstore).await }.err_into(),
            )
        })
        .collect();

    let (content, parents_content) = future::try_join(
        fetch_content_for_blame(ctx, repo, file_unode_id, options),
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

    store_blame(ctx, &blobstore, file_unode_id, blame_maybe_rejected).await
}
