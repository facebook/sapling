/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use blame::RootBlameV2;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use changeset_info::ChangesetInfo;
use context::CoreContext;
use filestore::FetchKey;
use fsnodes::RootFsnodeId;
use futures::stream::BoxStream;
use futures::try_join;
use futures::Stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use manifest::CombinedId;
use manifest::Diff;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::blame_v2::BlameV2;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::path::MPath;
use mononoke_types::BlameV2Id;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::FileUnodeId;
use mononoke_types::FsnodeId;
use mononoke_types::ManifestUnodeId;
use unodes::RootUnodeManifestId;

use crate::types::BlamedTextFileMetadata;
use crate::types::ChangeType;
use crate::types::DirectoryMetadata;
use crate::types::FileMetadata;
use crate::types::ItemHistory;
use crate::types::MetadataItem;
use crate::types::SymlinkMetadata;
use crate::types::TextFileMetadata;
use crate::Repo;
use crate::RepoMetadataLoggerMode;

/// Produces stream of file and directory metadata items for the given
/// bookmark in the given repo
pub async fn repo_metadata_for_bookmark<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    bookmark: &BookmarkKey,
    cs_id: ChangesetId,
    mode: RepoMetadataLoggerMode,
) -> Result<impl Stream<Item = Result<MetadataItem>> + 'a> {
    match mode {
        RepoMetadataLoggerMode::Full => process_changeset(ctx, repo, cs_id).await,
        RepoMetadataLoggerMode::Incremental => {
            let base_commit = repo
                .repo_metadata_checkpoint()
                .get_entry(bookmark.clone().into_string())
                .await?
                .map(|entry| entry.changeset_id);
            match base_commit {
                Some(base_commit) => {
                    process_changeset_with_base(ctx, repo, cs_id, base_commit).await
                }
                None => process_changeset(ctx, repo, cs_id).await,
            }
        }
    }
}

async fn fsnode_and_unode(
    ctx: &CoreContext,
    repo: &impl Repo,
    cs_id: ChangesetId,
) -> Result<(RootFsnodeId, RootUnodeManifestId)> {
    let (fsnode, blame) = try_join!(
        repo.repo_derived_data().derive::<RootFsnodeId>(ctx, cs_id),
        repo.repo_derived_data().derive::<RootBlameV2>(ctx, cs_id)
    )?;
    let unode = blame.root_manifest();
    Ok((fsnode, unode))
}

async fn process_changeset<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    cs_id: ChangesetId,
) -> Result<BoxStream<'a, Result<MetadataItem>>> {
    let (root_fsnode, root_unode) = fsnode_and_unode(ctx, repo, cs_id).await?;

    // Iterate over pairs of fsnodes and unodes for all files and directories. All
    // the metadata we want is either stored directly in fsnodes and unodes, or can
    // be fetched using the content id from fsnodes or the unode id.
    Ok(
        CombinedId(*root_fsnode.fsnode_id(), *root_unode.manifest_unode_id())
            .list_all_entries(ctx.clone(), repo.repo_blobstore_arc())
            .map_ok(|(path, entry)| process_entry(ctx, repo, path, ChangeType::Unknown, entry))
            .try_buffered(200)
            .boxed(),
    )
}

async fn process_changeset_with_base<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    new_cs_id: ChangesetId,
    old_cs_id: ChangesetId,
) -> Result<BoxStream<'a, Result<MetadataItem>>> {
    let (new_root_fsnode, new_root_unode) = fsnode_and_unode(ctx, repo, new_cs_id).await?;
    let (old_root_fsnode, old_root_unode) = fsnode_and_unode(ctx, repo, old_cs_id).await?;
    let new_manifest = CombinedId(
        *new_root_fsnode.fsnode_id(),
        *new_root_unode.manifest_unode_id(),
    );
    let old_manifest = CombinedId(
        *old_root_fsnode.fsnode_id(),
        *old_root_unode.manifest_unode_id(),
    );

    // Iterate over pairs of fsnodes and unodes for those files and directories that are different in new_cs_id
    // as compared to old_cs_id. All the metadata we want is either stored directly in fsnodes and unodes, or can
    // be fetched using the content id from fsnodes or the unode id.
    Ok(old_manifest
        .diff(ctx.clone(), repo.repo_blobstore_arc(), new_manifest)
        .map_ok(|diff_entry| match diff_entry {
            Diff::Added(path, entry) => process_entry(ctx, repo, path, ChangeType::Added, entry),
            Diff::Changed(path, _old_entry, new_entry) => {
                process_entry(ctx, repo, path, ChangeType::Modified, new_entry)
            }
            Diff::Removed(path, entry) => {
                process_entry(ctx, repo, path, ChangeType::Deleted, entry)
            }
        })
        .try_buffered(200)
        .boxed())
}

async fn process_entry(
    ctx: &CoreContext,
    repo: &impl Repo,
    path: MPath,
    change_type: ChangeType,
    entry: Entry<CombinedId<FsnodeId, ManifestUnodeId>, CombinedId<FsnodeFile, FileUnodeId>>,
) -> Result<MetadataItem> {
    match entry {
        Entry::Tree(CombinedId(fsnode_id, manifest_unode_id)) => {
            process_tree(ctx, repo, path, change_type, fsnode_id, manifest_unode_id).await
        }
        Entry::Leaf(CombinedId(fsnode_file, file_unode_id)) => {
            process_file(ctx, repo, path, change_type, fsnode_file, file_unode_id).await
        }
    }
}

async fn process_tree(
    ctx: &CoreContext,
    repo: &impl Repo,
    path: MPath,
    change_type: ChangeType,
    fsnode_id: FsnodeId,
    manifest_unode_id: ManifestUnodeId,
) -> Result<MetadataItem> {
    let (fsnode, manifest_unode) = try_join!(
        fsnode_id.load(ctx, repo.repo_blobstore()),
        manifest_unode_id.load(ctx, repo.repo_blobstore())
    )?;
    let summary = fsnode.summary();
    let info = repo
        .repo_derived_data()
        .derive::<ChangesetInfo>(ctx, *manifest_unode.linknode())
        .await?;

    Ok(MetadataItem::Directory(DirectoryMetadata {
        path,
        history: ItemHistory {
            last_author: info.author().to_string(),
            last_modified_timestamp: *info.author_date(),
        },
        child_files_count: summary.child_files_count,
        child_files_total_size: summary.child_files_total_size,
        child_dirs_count: summary.child_dirs_count,
        descendant_files_count: summary.descendant_files_count,
        descendant_files_total_size: summary.descendant_files_total_size,
        change_type,
    }))
}

async fn process_file(
    ctx: &CoreContext,
    repo: &impl Repo,
    path: MPath,
    change_type: ChangeType,
    fsnode_file: FsnodeFile,
    file_unode_id: FileUnodeId,
) -> Result<MetadataItem> {
    let blame_id = BlameV2Id::from(file_unode_id);
    let filestore_key = FetchKey::from(*fsnode_file.content_id());
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
            fsnode_file.content_id()
        )
    })?;
    let info = repo
        .repo_derived_data()
        .derive::<ChangesetInfo>(ctx, *file_unode.linknode())
        .await?;

    let file_metadata = FileMetadata::new(path, info, fsnode_file, change_type);

    if *fsnode_file.file_type() == FileType::Symlink {
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
