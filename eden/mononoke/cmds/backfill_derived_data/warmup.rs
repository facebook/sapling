/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blame::fetch_content_for_blame;
use blame::BlameRoot;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use context::CoreContext;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data::BonsaiDerived;
use fastlog::RootFastlog;
use fsnodes::prefetch_content_metadata;
use fsnodes::RootFsnodeId;
use futures::future;
use futures::future::try_join3;
use futures::future::try_join4;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::TryFutureExt;
use manifest::find_intersection_of_diffs;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use repo_derived_data::RepoDerivedDataRef;
use std::collections::HashSet;
use std::sync::Arc;
use unodes::find_unode_rename_sources;
use unodes::RootUnodeManifestId;

/// Types of derived data for which prefetching content for changed files
/// migth speed up derivation.
const PREFETCH_CONTENT_TYPES: &[&str] = &[BlameRoot::DERIVABLE_NAME];
const PREFETCH_CONTENT_METADATA_TYPES: &[&str] = &[RootFsnodeId::DERIVABLE_NAME];
const PREFETCH_UNODE_TYPES: &[&str] = &[
    RootFastlog::DERIVABLE_NAME,
    RootDeletedManifestV2Id::DERIVABLE_NAME,
];

pub(crate) async fn warmup(
    ctx: &CoreContext,
    repo: &BlobRepo,
    derived_data_type: &str,
    chunk: &Vec<ChangesetId>,
) -> Result<(), Error> {
    // Warmup bonsai changesets unconditionally because
    // most likely all derived data needs it. And they are cheap to warm up anyway

    let bcs_warmup = async move {
        stream::iter(chunk)
            .map(move |cs_id| Ok(async move { cs_id.load(ctx, repo.blobstore()).await }))
            .try_for_each_concurrent(100, |x| async {
                x.await?;
                Result::<_, Error>::Ok(())
            })
            .await
    };

    let content_warmup = async {
        if PREFETCH_CONTENT_TYPES.contains(&derived_data_type) {
            content_warmup(ctx, repo, chunk).await?
        }
        Ok(())
    };

    let metadata_warmup = async {
        if PREFETCH_CONTENT_METADATA_TYPES.contains(&derived_data_type) {
            content_metadata_warmup(ctx, repo, chunk).await?
        }
        Ok(())
    };

    let unode_warmup = async {
        if PREFETCH_UNODE_TYPES.contains(&derived_data_type) {
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
        .map(move |csid| Ok(prefetch_content(ctx, repo, csid)))
        .try_for_each_concurrent(100, |_| async { Ok(()) })
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
                let bcs = cs_id.load(ctx, repo.blobstore()).await?;

                let mut content_ids = HashSet::new();
                for (_, file_change) in bcs.simplified_file_changes() {
                    match file_change {
                        Some(fc) => {
                            content_ids.insert(fc.content_id());
                        }
                        None => {}
                    }
                }
                prefetch_content_metadata(ctx, repo.blobstore(), content_ids).await?;

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
    stream::iter(chunk)
        .map({
            |cs_id| {
                async move {
                    let bcs = cs_id.load(ctx, repo.blobstore()).await?;
                    let manager = repo.repo_derived_data().manager();

                    let root_mf_id = manager
                        .derive::<RootUnodeManifestId>(ctx, *cs_id, None)
                        .await?;
                    let parent_unodes = manager
                        .derivation_context(None)
                        .fetch_parents::<RootUnodeManifestId>(ctx, &bcs)
                        .await?
                        .into_iter()
                        .map(|id| id.manifest_unode_id().clone())
                        .collect();
                    let unode_mf_id = root_mf_id.manifest_unode_id().clone();
                    find_intersection_of_diffs(
                        ctx.clone(),
                        Arc::new(repo.get_blobstore()),
                        unode_mf_id,
                        parent_unodes,
                    )
                    .try_for_each(|_| async { Ok(()) })
                    .await?;

                    Result::<_, Error>::Ok(())
                }
                .map(|_: Result<(), _>| ()) // Ignore warm up failures
            }
        })
        .for_each_concurrent(100, |f| f)
        .await;
    Ok(())
}

// Prefetch content of changed files between parents
async fn prefetch_content(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csid: &ChangesetId,
) -> Result<(), Error> {
    async fn prefetch_content_unode(
        ctx: CoreContext,
        repo: BlobRepo,
        rename: Option<FileUnodeId>,
        file_unode_id: FileUnodeId,
    ) -> Result<(), Error> {
        let ctx = &ctx;
        let repo = &repo;
        let blobstore = repo.blobstore();
        let file_unode = file_unode_id.load(ctx, blobstore).await?;
        let parents_content: Vec<_> = file_unode
            .parents()
            .iter()
            .cloned()
            .chain(rename)
            .map(|file_unode_id| fetch_content_for_blame(ctx, repo, file_unode_id))
            .collect();

        // the assignment is needed to avoid unused_must_use warnings
        let _ = future::try_join(
            fetch_content_for_blame(ctx, repo, file_unode_id),
            future::try_join_all(parents_content),
        )
        .await?;
        Ok(())
    }

    let bonsai = csid.load(ctx, repo.blobstore()).await?;

    let root_manifest_fut = RootUnodeManifestId::derive(ctx, repo, csid.clone())
        .map_ok(|mf| mf.manifest_unode_id().clone())
        .map_err(Error::from);
    let parents_manifest_futs = bonsai.parents().collect::<Vec<_>>().into_iter().map({
        move |csid| {
            RootUnodeManifestId::derive(ctx, repo, csid)
                .map_ok(|mf| mf.manifest_unode_id().clone())
                .map_err(Error::from)
        }
    });
    let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);
    let (root_manifest, parents_manifests, renames) = try_join3(
        root_manifest_fut,
        future::try_join_all(parents_manifest_futs),
        find_unode_rename_sources(ctx, &derivation_ctx, &bonsai),
    )
    .await?;

    let blobstore = repo.get_blobstore().boxed();

    find_intersection_of_diffs(
        ctx.clone(),
        blobstore.clone(),
        root_manifest,
        parents_manifests,
    )
    .map_ok(|(path, entry)| Some((path?, entry.into_leaf()?)))
    .try_filter_map(|maybe_entry| async move { Result::<_, Error>::Ok(maybe_entry) })
    .map(|result| async {
        match result {
            Ok((path, file)) => {
                let rename_unode_id = renames.get(&path).map(|source| source.unode_id);
                let fut = prefetch_content_unode(ctx.clone(), repo.clone(), rename_unode_id, file);
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
