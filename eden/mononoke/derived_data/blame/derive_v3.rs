/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use filestore::FetchKey;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures::future;
use history_manifest::HmRenameSource;
use history_manifest::HmRenameSources;
use history_manifest::RootHistoryManifestDirectoryId;
use history_manifest::find_hm_rename_sources;
use itertools::Itertools;
use manifest::ManifestOps;
use manifest::find_intersection_of_diffs;
use mononoke_macros::mononoke;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::NonRootMPath;
use mononoke_types::blame_v2::BlameParent;
use mononoke_types::blame_v2::BlameParentId;
use mononoke_types::blame_v2::BlameRejected;
use mononoke_types::blame_v2::BlameV2;
use mononoke_types::blame_v3::BlameV3Id;
use mononoke_types::blame_v3::store_blame_v3;
use mononoke_types::history_manifest::HistoryManifestEntry;
use mononoke_types::history_manifest::HistoryManifestFile;
use mononoke_types::typed_hash::HistoryManifestFileId;

use crate::DEFAULT_BLAME_FILESIZE_LIMIT;
use crate::fetch::FetchOutcome;

pub(crate) async fn derive_blame_v3(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: BonsaiChangeset,
    root_manifest: RootHistoryManifestDirectoryId,
) -> Result<(), Error> {
    let csid = bonsai.get_changeset_id();
    let blobstore = derivation_ctx.blobstore();

    let parent_manifests = bonsai.parents().map(|csid| async move {
        let parent_root = derivation_ctx
            .fetch_dependency::<RootHistoryManifestDirectoryId>(ctx, csid)
            .await?;
        Ok::<_, Error>(parent_root.into_history_manifest_directory_id())
    });

    let (parent_manifests, renames) = future::try_join(
        future::try_join_all(parent_manifests).err_into(),
        find_hm_rename_sources(ctx, derivation_ctx, &bonsai),
    )
    .await?;

    let filesize_limit = derivation_ctx
        .config()
        .blame_filesize_limit
        .unwrap_or(DEFAULT_BLAME_FILESIZE_LIMIT);
    let renames = Arc::new(renames);
    // Paths the changeset itself modifies. A path in the history manifest diff
    // that is absent here is there purely because of merge resolution, which is
    // the case `create_blame_v3` can satisfy by reusing a parent's blame.
    let changed_paths: Arc<HashSet<NonRootMPath>> = Arc::new(
        bonsai
            .file_changes()
            .map(|(path, _)| path.clone())
            .collect(),
    );
    find_intersection_of_diffs(
        ctx.clone(),
        blobstore.clone(),
        root_manifest.into_history_manifest_directory_id(),
        parent_manifests,
    )
    .map_ok(|(path, entry)| Some((Option::<NonRootMPath>::from(path)?, entry.into_leaf()?)))
    .try_filter_map(future::ok)
    .map(move |path_and_hm_file| {
        cloned!(ctx, derivation_ctx, blobstore, renames, changed_paths);
        async move {
            let (path, hm_file_id) = path_and_hm_file?;
            mononoke::spawn_task(async move {
                create_blame_v3(
                    &ctx,
                    &derivation_ctx,
                    &blobstore,
                    renames,
                    changed_paths,
                    csid,
                    path,
                    hm_file_id,
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

async fn create_blame_v3(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    renames: Arc<HmRenameSources>,
    changed_paths: Arc<HashSet<NonRootMPath>>,
    csid: ChangesetId,
    path: NonRootMPath,
    hm_file_id: HistoryManifestFileId,
    filesize_limit: u64,
) -> Result<BlameV3Id, Error> {
    let hm_file = hm_file_id.load(ctx, blobstore).await?;

    if let Some(blame_id) = reuse_parent_blame_v3(
        ctx,
        blobstore,
        &changed_paths,
        &renames,
        &path,
        hm_file_id,
        &hm_file,
    )
    .await?
    {
        return Ok(blame_id);
    }

    let mut blame_parents = Vec::new();
    for (parent_index, parent_entry) in hm_file.parents.iter().enumerate() {
        if let HistoryManifestEntry::File(parent_hm_file_id) = parent_entry {
            blame_parents.push(fetch_blame_parent_v3(
                ctx,
                derivation_ctx,
                blobstore,
                BlameV3ParentSource::FileParent {
                    parent_index,
                    hm_file_id: *parent_hm_file_id,
                },
                path.clone(),
                filesize_limit,
            ));
        }
    }

    if let Some(source) = renames.get(&path) {
        match source {
            HmRenameSource::CopyInfo(source) => {
                blame_parents.clear();
                blame_parents.push(fetch_blame_parent_v3(
                    ctx,
                    derivation_ctx,
                    blobstore,
                    BlameV3ParentSource::FileParent {
                        parent_index: source.parent_index,
                        hm_file_id: source.history_manifest_file_id,
                    },
                    source.from_path.clone(),
                    filesize_limit,
                ));
            }
            HmRenameSource::SubtreeCopy(copy) => {
                blame_parents.clear();
                blame_parents.push(fetch_blame_parent_v3(
                    ctx,
                    derivation_ctx,
                    blobstore,
                    BlameV3ParentSource::ReplacementParent(copy.parent),
                    copy.from_path
                        .into_optional_non_root_path()
                        .ok_or_else(|| anyhow!("Copy source must be a file"))?,
                    filesize_limit,
                ));
            }
            HmRenameSource::SubtreeMerge(merge) => {
                blame_parents.push(fetch_blame_parent_v3(
                    ctx,
                    derivation_ctx,
                    blobstore,
                    BlameV3ParentSource::ReplacementParent(merge.parent),
                    merge
                        .from_path
                        .into_optional_non_root_path()
                        .ok_or_else(|| anyhow!("Merge source must be a file"))?,
                    filesize_limit,
                ));
            }
        }
    }

    let (content, blame_parents) = future::try_join(
        fetch_content_by_content_id(ctx, blobstore, hm_file.content_id, filesize_limit),
        future::try_join_all(blame_parents),
    )
    .await?;

    let blame_parents = blame_parents.into_iter().flatten().collect();

    let blame = match content {
        FetchOutcome::Rejected(rejected) => BlameV2::rejected(rejected),
        FetchOutcome::Fetched(content) => BlameV2::new(csid, path, content, blame_parents)?,
    };

    store_blame_v3(ctx, blobstore, hm_file_id, blame).await
}

/// Reuse a parent's blame verbatim when this file node exists only because of
/// merge resolution and its content is exactly one parent's.
///
/// A merge that re-emits a file only one parent still has (deleted on the
/// other) makes the history manifest create a fresh file node even though the
/// file version is unchanged. Recomputing blame there is not just wasted work,
/// it degrades the result: the content matches the parent's, so no line is
/// attributed to this changeset, but the changeset has already consumed a csid
/// index. `compact` then strips the unreferenced entry while `max_csid_index`
/// keeps counting it. Since a child takes `parent.max_csid_index + 1`, every
/// later version of the file inherits that gap, and the index space drifts
/// further from the number of changesets actually blamed with each such merge.
///
/// Deliberately narrow — each condition below marks a case where the blame
/// genuinely differs from the parent's and must be computed:
///   * the changeset modifies the path, so its content and linknode are new;
///   * two live parents, so the merge resolves between differing versions;
///   * a rename or subtree source, so the blame parent is not the manifest
///     parent.
async fn reuse_parent_blame_v3(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    changed_paths: &HashSet<NonRootMPath>,
    renames: &HmRenameSources,
    path: &NonRootMPath,
    hm_file_id: HistoryManifestFileId,
    hm_file: &HistoryManifestFile,
) -> Result<Option<BlameV3Id>, Error> {
    if changed_paths.contains(path) || renames.get(path).is_some() {
        return Ok(None);
    }

    let Ok(parent_hm_file_id) = hm_file
        .parents
        .iter()
        .filter_map(|parent| match parent {
            HistoryManifestEntry::File(parent_hm_file_id) => Some(*parent_hm_file_id),
            _ => None,
        })
        .exactly_one()
    else {
        return Ok(None);
    };

    let parent_hm_file = parent_hm_file_id.load(ctx, blobstore).await?;
    if parent_hm_file.content_id != hm_file.content_id
        || parent_hm_file.file_type != hm_file.file_type
    {
        return Ok(None);
    }

    let parent_blame = BlameV3Id::from(parent_hm_file_id)
        .load(ctx, blobstore)
        .await?;
    Ok(Some(
        store_blame_v3(ctx, blobstore, hm_file_id, parent_blame).await?,
    ))
}

enum BlameV3ParentSource {
    /// The source of this blame parent is a file in a parent manifest.
    FileParent {
        parent_index: usize,
        hm_file_id: HistoryManifestFileId,
    },
    /// The source of this blame parent is a replacement parent (e.g. due
    /// to a subtree operation).
    ReplacementParent(ChangesetId),
}

async fn fetch_blame_parent_v3(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    parent_info: BlameV3ParentSource,
    path: NonRootMPath,
    filesize_limit: u64,
) -> Result<Option<BlameParent<Bytes>>, Error> {
    let (parent, hm_file_id) = match parent_info {
        BlameV3ParentSource::FileParent {
            parent_index,
            hm_file_id,
        } => (BlameParentId::ChangesetParent(parent_index), hm_file_id),
        BlameV3ParentSource::ReplacementParent(csid) => {
            let root = derivation_ctx
                .fetch_dependency::<RootHistoryManifestDirectoryId>(ctx, csid)
                .await?;
            let entry = root
                .into_history_manifest_directory_id()
                .find_entry(ctx.clone(), blobstore.clone(), path.clone().into())
                .await?;
            let hm_file_id = match entry.and_then(|e| e.into_leaf()) {
                Some(id) => id,
                None => return Ok(None),
            };
            (BlameParentId::ReplacementParent(csid), hm_file_id)
        }
    };

    let hm_file = hm_file_id.load(ctx, blobstore).await?;
    let (content, blame) = future::try_join(
        fetch_content_by_content_id(ctx, blobstore, hm_file.content_id, filesize_limit),
        BlameV3Id::from(hm_file_id).load(ctx, blobstore).err_into(),
    )
    .await?;

    Ok(Some(BlameParent::new(
        parent,
        path,
        content.into_bytes().ok(),
        blame,
    )))
}

/// Fetch file content by ContentId, checking size limit and binary content.
async fn fetch_content_by_content_id(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    content_id: ContentId,
    filesize_limit: u64,
) -> Result<FetchOutcome> {
    let (mut stream, size) =
        filestore::fetch_with_size(blobstore.clone(), ctx, &FetchKey::Canonical(content_id))
            .await?
            .ok_or_else(|| anyhow!("Missing content: {content_id}"))?;
    if size > filesize_limit {
        return Ok(FetchOutcome::Rejected(BlameRejected::TooBig));
    }
    let mut buffer = Vec::with_capacity(size as usize);
    while let Some(bytes) = stream.try_next().await? {
        if bytes.contains(&0u8) {
            return Ok(FetchOutcome::Rejected(BlameRejected::Binary));
        }
        buffer.extend(bytes);
    }
    Ok(FetchOutcome::Fetched(Bytes::from(buffer)))
}
