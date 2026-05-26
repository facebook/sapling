/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::anyhow;
use blame::RootBlameV2;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkName;
use changeset_info::ChangesetInfo;
use content_manifest_derivation::RootContentManifestId;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
use filestore::FetchKey;
use fsnodes::RootFsnodeId;
use futures::Stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures::stream::BoxStream;
use futures::try_join;
use manifest::CombinedId;
use manifest::Diff;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::BlameV2Id;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::FileUnodeId;
use mononoke_types::ManifestUnodeId;
use mononoke_types::blame_v2::BlameV2;
use mononoke_types::content_manifest::ContentManifestFile;
use mononoke_types::content_manifest::compat;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::path::MPath;
use unodes::RootUnodeManifestId;

use crate::Repo;
use crate::RepoMetadataLoggerMode;
use crate::types::BlamedTextFileMetadata;
use crate::types::ChangeType;
use crate::types::DirectoryMetadata;
use crate::types::FileMetadata;
use crate::types::ItemHistory;
use crate::types::MetadataItem;
use crate::types::SymlinkMetadata;
use crate::types::TextFileMetadata;

/// Produces stream of file and directory metadata items for the given
/// bookmark in the given repo
pub async fn repo_metadata_for_bookmark<'a, T: Repo>(
    ctx: &'a CoreContext,
    repo: &'a T,
    bookmark: &'a BookmarkKey,
    cs_id: ChangesetId,
    mode: RepoMetadataLoggerMode,
) -> Result<impl Stream<Item = Result<MetadataItem>> + use<'a, T>> {
    let bookmark_name = bookmark.name();
    match mode {
        RepoMetadataLoggerMode::Full => process_changeset(ctx, repo, bookmark_name, cs_id).await,
        RepoMetadataLoggerMode::Incremental => {
            let base_commit = repo
                .repo_metadata_checkpoint()
                .get_entry(bookmark.clone().into_string())
                .await?
                .map(|entry| entry.changeset_id);
            match base_commit {
                Some(base_commit) => {
                    process_changeset_with_base(ctx, repo, bookmark_name, cs_id, base_commit).await
                }
                None => process_changeset(ctx, repo, bookmark_name, cs_id).await,
            }
        }
    }
}

async fn manifest_and_unode(
    ctx: &CoreContext,
    repo: &impl Repo,
    cs_id: ChangesetId,
) -> Result<(compat::ContentManifestId, RootUnodeManifestId)> {
    let repo_name = repo.repo_identity().name();
    let use_content_manifests = justknobs::eval(
        "scm/mononoke:derived_data_use_content_manifests",
        None,
        Some(repo_name),
    )?;

    let (root, unode) = if use_content_manifests {
        let (content_manifest, blame) = try_join!(
            repo.repo_derived_data().derive::<RootContentManifestId>(
                ctx,
                cs_id,
                DerivationPriority::LOW
            ),
            repo.repo_derived_data()
                .derive::<RootBlameV2>(ctx, cs_id, DerivationPriority::LOW)
        )?;
        let root: compat::ContentManifestId = content_manifest.into_content_manifest_id().into();
        (root, blame.root_manifest())
    } else {
        let (fsnode, blame) = try_join!(
            repo.repo_derived_data()
                .derive::<RootFsnodeId>(ctx, cs_id, DerivationPriority::LOW),
            repo.repo_derived_data()
                .derive::<RootBlameV2>(ctx, cs_id, DerivationPriority::LOW)
        )?;
        let root: compat::ContentManifestId = fsnode.into_fsnode_id().into();
        (root, blame.root_manifest())
    };
    Ok((root, unode))
}

async fn process_changeset<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    bookmark: &'a BookmarkName,
    cs_id: ChangesetId,
) -> Result<BoxStream<'a, Result<MetadataItem>>> {
    let (root_manifest, root_unode) = manifest_and_unode(ctx, repo, cs_id).await?;

    // Iterate over pairs of content manifests (or fsnodes) and unodes for all files
    // and directories. All the metadata we want is either stored directly in the
    // manifest and unodes, or can be fetched using the content id or the unode id.
    Ok(CombinedId(root_manifest, *root_unode.manifest_unode_id())
        .list_all_entries(ctx.clone(), repo.repo_blobstore_arc())
        .map_ok(|(path, entry)| {
            process_entry(ctx, repo, bookmark, path, ChangeType::Unknown, entry)
        })
        .try_buffered(200)
        .boxed())
}

async fn process_changeset_with_base<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    bookmark: &'a BookmarkName,
    new_cs_id: ChangesetId,
    old_cs_id: ChangesetId,
) -> Result<BoxStream<'a, Result<MetadataItem>>> {
    let ((new_root_manifest, new_root_unode), (old_root_manifest, old_root_unode)) = try_join!(
        manifest_and_unode(ctx, repo, new_cs_id),
        manifest_and_unode(ctx, repo, old_cs_id),
    )?;
    let new_manifest = CombinedId(new_root_manifest, *new_root_unode.manifest_unode_id());
    let old_manifest = CombinedId(old_root_manifest, *old_root_unode.manifest_unode_id());

    // Iterate over pairs of content manifests (or fsnodes) and unodes for those files
    // and directories that are different in new_cs_id as compared to old_cs_id.
    Ok(old_manifest
        .diff(ctx.clone(), repo.repo_blobstore_arc(), new_manifest)
        .map_ok(|diff_entry| match diff_entry {
            Diff::Added(path, entry) => {
                process_entry(ctx, repo, bookmark, path, ChangeType::Added, entry)
            }
            Diff::Changed(path, _old_entry, new_entry) => {
                process_entry(ctx, repo, bookmark, path, ChangeType::Modified, new_entry)
            }
            Diff::Removed(path, entry) => {
                process_entry(ctx, repo, bookmark, path, ChangeType::Deleted, entry)
            }
        })
        .try_buffered(200)
        .boxed())
}

async fn process_entry(
    ctx: &CoreContext,
    repo: &impl Repo,
    bookmark: &BookmarkName,
    path: MPath,
    change_type: ChangeType,
    entry: Entry<
        CombinedId<compat::ContentManifestId, ManifestUnodeId>,
        CombinedId<either::Either<ContentManifestFile, FsnodeFile>, FileUnodeId>,
    >,
) -> Result<MetadataItem> {
    match entry {
        Entry::Tree(CombinedId(tree_id, manifest_unode_id)) => {
            process_tree(
                ctx,
                repo,
                bookmark,
                path,
                change_type,
                tree_id,
                manifest_unode_id,
            )
            .await
        }
        Entry::Leaf(CombinedId(leaf_id, file_unode_id)) => {
            let manifest_file: compat::ContentManifestFile = leaf_id.into();
            process_file(
                ctx,
                repo,
                bookmark,
                path,
                change_type,
                manifest_file,
                file_unode_id,
            )
            .await
        }
    }
}

async fn process_tree(
    ctx: &CoreContext,
    repo: &impl Repo,
    bookmark: &BookmarkName,
    path: MPath,
    change_type: ChangeType,
    tree_id: compat::ContentManifestId,
    manifest_unode_id: ManifestUnodeId,
) -> Result<MetadataItem> {
    let manifest_unode = manifest_unode_id.load(ctx, repo.repo_blobstore()).await?;
    let info = repo
        .repo_derived_data()
        .derive::<ChangesetInfo>(ctx, *manifest_unode.linknode(), DerivationPriority::LOW)
        .await?;

    let (
        child_files_count,
        child_files_total_size,
        child_dirs_count,
        descendant_files_count,
        descendant_files_total_size,
    ) = match tree_id {
        either::Either::Left(content_manifest_id) => {
            let content_manifest = content_manifest_id.load(ctx, repo.repo_blobstore()).await?;
            let rollup = content_manifest.subentries.rollup_data();
            (
                rollup.child_counts.files_count,
                rollup.child_counts.files_total_size,
                rollup.child_counts.dirs_count,
                rollup.descendant_counts.files_count,
                rollup.descendant_counts.files_total_size,
            )
        }
        either::Either::Right(fsnode_id) => {
            let fsnode = fsnode_id.load(ctx, repo.repo_blobstore()).await?;
            let summary = fsnode.summary();
            (
                summary.child_files_count,
                summary.child_files_total_size,
                summary.child_dirs_count,
                summary.descendant_files_count,
                summary.descendant_files_total_size,
            )
        }
    };

    Ok(MetadataItem::Directory(DirectoryMetadata {
        path,
        bookmark: bookmark.clone(),
        history: ItemHistory {
            last_author: info.author().to_string(),
            last_modified_timestamp: *info.author_date(),
        },
        child_files_count,
        child_files_total_size,
        child_dirs_count,
        descendant_files_count,
        descendant_files_total_size,
        change_type,
    }))
}

async fn process_file(
    ctx: &CoreContext,
    repo: &impl Repo,
    bookmark: &BookmarkName,
    path: MPath,
    change_type: ChangeType,
    manifest_file: compat::ContentManifestFile,
    file_unode_id: FileUnodeId,
) -> Result<MetadataItem> {
    let blame_id = BlameV2Id::from(file_unode_id);
    let filestore_key = FetchKey::from(manifest_file.content_id());
    let (file_unode, blame, content_metadata) = try_join!(
        file_unode_id
            .load(ctx, repo.repo_blobstore())
            .map_err(anyhow::Error::from),
        blame_id
            .load(ctx, repo.repo_blobstore())
            .map_err(anyhow::Error::from),
        filestore::get_metadata(repo.repo_blobstore(), ctx, &filestore_key),
    )?;
    let content_metadata = content_metadata.ok_or_else(|| {
        anyhow!(
            "Can't get content metadata for id: {:?}",
            manifest_file.content_id()
        )
    })?;
    let info = repo
        .repo_derived_data()
        .derive::<ChangesetInfo>(ctx, *file_unode.linknode(), DerivationPriority::LOW)
        .await?;

    let file_type = manifest_file.file_type();
    let file_metadata = FileMetadata::new(path, bookmark.clone(), info, manifest_file, change_type);

    if file_type == FileType::Symlink {
        let content = filestore::fetch_concat(repo.repo_blobstore(), ctx, filestore_key).await?;
        return Ok(MetadataItem::Symlink(SymlinkMetadata::new(
            file_metadata,
            content,
        )));
    }

    if content_metadata.is_binary {
        return Ok(MetadataItem::BinaryFile(file_metadata));
    }

    let text_file_metadata = TextFileMetadata::new(file_metadata, content_metadata);

    match blame {
        BlameV2::Rejected(_) => Ok(MetadataItem::TextFile(text_file_metadata)),
        BlameV2::Blame(blame) => Ok(MetadataItem::BlamedTextFile(
            BlamedTextFileMetadata::new(ctx, repo, text_file_metadata, blame).await?,
        )),
    }
}
