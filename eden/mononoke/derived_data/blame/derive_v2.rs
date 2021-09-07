/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Error, Result};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use futures::{future, StreamExt, TryFutureExt, TryStreamExt};
use manifest::find_intersection_of_diffs;
use mononoke_types::blame_v2::{store_blame, BlameParent, BlameV2, BlameV2Id};
use mononoke_types::{BonsaiChangeset, ChangesetId, FileUnodeId, MPath};
use repo_blobstore::RepoBlobstore;
use std::collections::HashMap;
use std::sync::Arc;
use unodes::{find_unode_rename_sources, RootUnodeManifestId, UnodeRenameSource};

use crate::fetch::{fetch_content_for_blame_with_options, FetchOutcome};
use crate::BlameDeriveOptions;

pub(crate) async fn derive_blame_v2(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bonsai: BonsaiChangeset,
    root_manifest: RootUnodeManifestId,
    options: &BlameDeriveOptions,
) -> Result<(), Error> {
    let blame_options = *options;
    let csid = bonsai.get_changeset_id();
    let parent_manifests = bonsai.parents().map(|csid| async move {
        let parent_root_mf_id = RootUnodeManifestId::derive(&ctx, &repo, csid).await?;
        Ok::<_, Error>(parent_root_mf_id.manifest_unode_id().clone())
    });

    let (parent_manifests, renames) = future::try_join(
        future::try_join_all(parent_manifests).err_into(),
        find_unode_rename_sources(ctx, repo, &bonsai),
    )
    .await?;

    let renames = Arc::new(renames);
    let blobstore = repo.get_blobstore();
    find_intersection_of_diffs(
        ctx.clone(),
        blobstore.clone(),
        root_manifest.manifest_unode_id().clone(),
        parent_manifests,
    )
    .map_ok(|(path, entry)| Some((path?, entry.into_leaf()?)))
    .try_filter_map(future::ok)
    .map(move |path_and_file_unode| {
        cloned!(ctx, blobstore, renames);
        async move {
            let (path, file_unode) = path_and_file_unode?;
            Ok::<_, Error>(
                tokio::spawn(async move {
                    create_blame_v2(
                        &ctx,
                        &blobstore,
                        renames,
                        csid,
                        path,
                        file_unode,
                        blame_options,
                    )
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

async fn create_blame_v2(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    renames: Arc<HashMap<MPath, UnodeRenameSource>>,
    csid: ChangesetId,
    path: MPath,
    file_unode_id: FileUnodeId,
    options: BlameDeriveOptions,
) -> Result<BlameV2Id, Error> {
    let file_unode = file_unode_id.load(ctx, blobstore).await?;

    let mut blame_parents = Vec::new();
    for (parent_index, unode_id) in file_unode.parents().iter().enumerate() {
        blame_parents.push(fetch_blame_parent(
            ctx,
            blobstore,
            parent_index,
            path.clone(),
            *unode_id,
            options,
        ));
    }

    if let Some(source) = renames.get(&path) {
        blame_parents.push(fetch_blame_parent(
            ctx,
            blobstore,
            source.parent_index,
            source.from_path.clone(),
            source.unode_id,
            options,
        ));
    }

    let (content, blame_parents) = future::try_join(
        fetch_content_for_blame_with_options(ctx, blobstore, file_unode_id, options),
        future::try_join_all(blame_parents),
    )
    .await?;

    let blame = match content {
        FetchOutcome::Rejected(rejected) => BlameV2::rejected(rejected),
        FetchOutcome::Fetched(content) => BlameV2::new(csid, path, content, blame_parents)?,
    };

    store_blame(ctx, &blobstore, file_unode_id, blame).await
}

async fn fetch_blame_parent(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    parent_index: usize,
    path: MPath,
    unode_id: FileUnodeId,
    options: BlameDeriveOptions,
) -> Result<BlameParent<Bytes>, Error> {
    let (content, blame) = future::try_join(
        fetch_content_for_blame_with_options(ctx, blobstore, unode_id, options),
        BlameV2Id::from(unode_id).load(ctx, blobstore).err_into(),
    )
    .await?;

    Ok(BlameParent::new(
        parent_index,
        path,
        content.into_bytes().ok(),
        blame,
    ))
}
