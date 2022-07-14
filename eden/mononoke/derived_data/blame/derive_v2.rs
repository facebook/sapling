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
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::future;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use manifest::find_intersection_of_diffs;
use mononoke_types::blame_v2::store_blame;
use mononoke_types::blame_v2::BlameParent;
use mononoke_types::blame_v2::BlameV2;
use mononoke_types::blame_v2::BlameV2Id;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::MPath;
use std::collections::HashMap;
use std::sync::Arc;
use unodes::find_unode_rename_sources;
use unodes::RootUnodeManifestId;
use unodes::UnodeRenameSource;

use crate::fetch::fetch_content_for_blame_with_limit;
use crate::fetch::FetchOutcome;
use crate::DEFAULT_BLAME_FILESIZE_LIMIT;

pub(crate) async fn derive_blame_v2(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: BonsaiChangeset,
    root_manifest: RootUnodeManifestId,
) -> Result<(), Error> {
    let csid = bonsai.get_changeset_id();
    let parent_manifests = bonsai.parents().map(|csid| async move {
        let parent_root_mf_id = derivation_ctx
            .derive_dependency::<RootUnodeManifestId>(ctx, csid)
            .await?;
        Ok::<_, Error>(parent_root_mf_id.manifest_unode_id().clone())
    });

    let (parent_manifests, renames) = future::try_join(
        future::try_join_all(parent_manifests).err_into(),
        find_unode_rename_sources(ctx, derivation_ctx, &bonsai),
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
        parent_manifests,
    )
    .map_ok(|(path, entry)| Some((path?, entry.into_leaf()?)))
    .try_filter_map(future::ok)
    .map(move |path_and_file_unode| {
        cloned!(ctx, blobstore, renames);
        async move {
            let (path, file_unode) = path_and_file_unode?;
            tokio::spawn(async move {
                create_blame_v2(
                    &ctx,
                    &blobstore,
                    renames,
                    csid,
                    path,
                    file_unode,
                    filesize_limit,
                )
                .await
            })
            .await?
        }
    })
    .buffered(256)
    .try_for_each(|_| future::ok(()))
    .await?;

    Ok(())
}

async fn create_blame_v2(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    renames: Arc<HashMap<MPath, UnodeRenameSource>>,
    csid: ChangesetId,
    path: MPath,
    file_unode_id: FileUnodeId,
    filesize_limit: u64,
) -> Result<BlameV2Id, Error> {
    let file_unode = file_unode_id.load(ctx, blobstore).await?;

    let mut blame_parents = Vec::new();
    if let Some(source) = renames.get(&path) {
        // If the file was copied from another path, then we ignore its
        // contents in the parents, even if it existed there, and just use the
        // copy-from source as a parent.  This matches the Mercurial blame
        // implementation.
        blame_parents.push(fetch_blame_parent(
            ctx,
            blobstore,
            source.parent_index,
            source.from_path.clone(),
            source.unode_id,
            filesize_limit,
        ));
    } else {
        for (parent_index, unode_id) in file_unode.parents().iter().enumerate() {
            blame_parents.push(fetch_blame_parent(
                ctx,
                blobstore,
                parent_index,
                path.clone(),
                *unode_id,
                filesize_limit,
            ));
        }
    }

    let (content, blame_parents) = future::try_join(
        fetch_content_for_blame_with_limit(ctx, blobstore, file_unode_id, filesize_limit),
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
    blobstore: &Arc<dyn Blobstore>,
    parent_index: usize,
    path: MPath,
    unode_id: FileUnodeId,
    filesize_limit: u64,
) -> Result<BlameParent<Bytes>, Error> {
    let (content, blame) = future::try_join(
        fetch_content_for_blame_with_limit(ctx, blobstore, unode_id, filesize_limit),
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
