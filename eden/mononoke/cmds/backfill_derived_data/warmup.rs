/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blame::{fetch_file_full_content, BlameRoot};
use blobrepo::BlobRepo;
use blobstore::{Blobstore, Loadable};
use cloned::cloned;
use context::CoreContext;
use deleted_files_manifest::RootDeletedManifestId;
use derived_data::BonsaiDerived;
use fastlog::{fetch_parent_root_unodes, RootFastlog};
use fsnodes::{prefetch_content_metadata, RootFsnodeId};
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{self, ready, try_join, try_join3, try_join4},
    stream::{self, FuturesUnordered, StreamExt, TryStreamExt},
    TryFutureExt,
};
use manifest::find_intersection_of_diffs;
use mononoke_types::{ChangesetId, FileUnodeId};
use std::{collections::HashSet, sync::Arc};
use unodes::{find_unode_renames, RootUnodeManifestId};

/// Types of derived data for which prefetching content for changed files
/// migth speed up derivation.
const PREFETCH_CONTENT_TYPES: &[&str] = &[BlameRoot::NAME];
const PREFETCH_CONTENT_METADATA_TYPES: &[&str] = &[RootFsnodeId::NAME];
const PREFETCH_UNODE_TYPES: &[&str] = &[RootFastlog::NAME, RootDeletedManifestId::NAME];

pub(crate) async fn warmup(
    ctx: &CoreContext,
    repo: &BlobRepo,
    derived_data_type: &String,
    chunk: &Vec<ChangesetId>,
) -> Result<(), Error> {
    // Warmup bonsai changesets unconditionally because
    // most likely all derived data needs it. And they are cheap to warm up anyway

    let bcs_warmup = {
        cloned!(ctx, chunk, repo);
        async move {
            stream::iter(chunk)
                .map(move |cs_id| Ok(cs_id.load(ctx.clone(), repo.blobstore()).compat()))
                .try_for_each_concurrent(100, |x| async {
                    x.await?;
                    Result::<_, Error>::Ok(())
                })
                .await
        }
    };

    let content_warmup = async {
        if PREFETCH_CONTENT_TYPES.contains(&derived_data_type.as_ref()) {
            content_warmup(ctx, repo, chunk).await?
        }
        Ok(())
    };

    let metadata_warmup = async {
        if PREFETCH_CONTENT_METADATA_TYPES.contains(&derived_data_type.as_ref()) {
            content_metadata_warmup(ctx, repo, chunk).await?
        }
        Ok(())
    };

    let unode_warmup = async {
        if PREFETCH_UNODE_TYPES.contains(&derived_data_type.as_ref()) {
            unode_warmup(ctx, repo, chunk).await?
        }
        Ok(())
    };

    try_join4(bcs_warmup, content_warmup, metadata_warmup, unode_warmup).await?;

    Ok(())
}

async fn content_warmup(
    ctx: &CoreContext,
    repo: &BlobRepo,
    chunk: &Vec<ChangesetId>,
) -> Result<(), Error> {
    stream::iter(chunk)
        .map(move |csid| prefetch_content(ctx, repo, csid))
        .buffered(crate::CHUNK_SIZE)
        .try_for_each(|_| async { Ok(()) })
        .await
}

async fn content_metadata_warmup(
    ctx: &CoreContext,
    repo: &BlobRepo,
    chunk: &Vec<ChangesetId>,
) -> Result<(), Error> {
    stream::iter(chunk)
        .map({
            |cs_id| async move {
                let bcs = cs_id.load(ctx.clone(), repo.blobstore()).compat().await?;

                let mut content_ids = HashSet::new();
                for (_, maybe_file_change) in bcs.file_changes() {
                    if let Some(file_change) = maybe_file_change {
                        content_ids.insert(file_change.content_id());
                    }
                }
                prefetch_content_metadata(ctx.clone(), repo.blobstore().clone(), content_ids)
                    .compat()
                    .await?;

                Result::<_, Error>::Ok(())
            }
        })
        .map(Result::<_, Error>::Ok)
        .try_for_each_concurrent(100, |f| f)
        .await?;
    Ok(())
}

async fn unode_warmup(
    ctx: &CoreContext,
    repo: &BlobRepo,
    chunk: &Vec<ChangesetId>,
) -> Result<(), Error> {
    let futs = FuturesUnordered::new();
    for cs_id in chunk {
        cloned!(ctx, repo);
        let f = async move {
            let bcs = cs_id.load(ctx.clone(), repo.blobstore()).compat().await?;

            let root_mf_id =
                RootUnodeManifestId::derive(ctx.clone(), repo.clone(), bcs.get_changeset_id())
                    .compat()
                    .map_err(Error::from);

            let parent_unodes = fetch_parent_root_unodes(ctx.clone(), repo.clone(), bcs);
            let (root_mf_id, parent_unodes) = try_join(root_mf_id, parent_unodes.compat()).await?;
            let unode_mf_id = root_mf_id.manifest_unode_id().clone();
            find_intersection_of_diffs(
                ctx.clone(),
                Arc::new(repo.get_blobstore()),
                unode_mf_id,
                parent_unodes,
            )
            .compat()
            .try_for_each(|_| async { Ok(()) })
            .await
        };
        futs.push(f);
    }

    futs.try_for_each(|_| ready(Ok(()))).await
}

// Prefetch content of changed files between parents
async fn prefetch_content(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csid: &ChangesetId,
) -> Result<(), Error> {
    async fn prefetch_content_unode<'a>(
        ctx: CoreContext,
        blobstore: Arc<dyn Blobstore>,
        rename: Option<FileUnodeId>,
        file_unode_id: FileUnodeId,
    ) -> Result<(), Error> {
        let ctx = &ctx;
        let file_unode = file_unode_id.load(ctx.clone(), &blobstore).compat().await?;
        let parents_content: Vec<_> = file_unode
            .parents()
            .iter()
            .cloned()
            .chain(rename)
            .map({
                cloned!(blobstore);
                move |file_unode_id| {
                    fetch_file_full_content(ctx.clone(), blobstore.clone(), file_unode_id).compat()
                }
            })
            .collect();

        // the assignment is needed to avoid unused_must_use warnings
        let _ = future::try_join(
            fetch_file_full_content(ctx.clone(), blobstore.clone(), file_unode_id).compat(),
            future::try_join_all(parents_content),
        )
        .await?;
        Ok(())
    }

    let bonsai = csid.load(ctx.clone(), repo.blobstore()).compat().await?;

    let root_manifest_fut = RootUnodeManifestId::derive(ctx.clone(), repo.clone(), csid.clone())
        .compat()
        .map_ok(|mf| mf.manifest_unode_id().clone())
        .map_err(Error::from);
    let parents_manifest_futs = bonsai.parents().collect::<Vec<_>>().into_iter().map({
        move |csid| {
            RootUnodeManifestId::derive(ctx.clone(), repo.clone(), csid)
                .compat()
                .map_ok(|mf| mf.manifest_unode_id().clone())
                .map_err(Error::from)
        }
    });
    let (root_manifest, parents_manifests, renames) = try_join3(
        root_manifest_fut,
        future::try_join_all(parents_manifest_futs),
        find_unode_renames(ctx.clone(), repo.clone(), &bonsai).compat(),
    )
    .await?;

    let blobstore = repo.get_blobstore().boxed();

    find_intersection_of_diffs(
        ctx.clone(),
        blobstore.clone(),
        root_manifest,
        parents_manifests,
    )
    .compat()
    .map_ok(|(path, entry)| Some((path?, entry.into_leaf()?)))
    .try_filter_map(|maybe_entry| async move { Result::<_, Error>::Ok(maybe_entry) })
    .map(|result| async {
        match result {
            Ok((path, file)) => {
                let rename = renames.get(&path).copied();
                let fut = prefetch_content_unode(ctx.clone(), blobstore.clone(), rename, file);
                let join_handle = tokio::task::spawn(fut);
                join_handle.await?
            }
            Err(e) => Err(e),
        }
    })
    .buffered(256)
    .try_for_each(|()| future::ready(Ok(())))
    .await
}
