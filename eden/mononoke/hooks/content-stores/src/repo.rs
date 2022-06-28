/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Context as _;
use async_trait::async_trait;
use blobstore::Loadable;
use bookmarks::ArcBookmarks;
use bookmarks::BookmarkName;
use bookmarks::BookmarksArc;
use bytes::Bytes;
use changeset_info::ChangesetInfo;
use context::CoreContext;
use futures::future;
use futures::stream::TryStreamExt;
use futures_util::future::TryFutureExt;
use manifest::Diff;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_derived_data::MappedHgChangesetId;
use mercurial_types::FileType;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::MPath;
use mononoke_types::ManifestUnodeId;
use repo_blobstore::ArcRepoBlobstore;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::ArcRepoDerivedData;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataArc;
use std::collections::HashMap;
use unodes::RootUnodeManifestId;

use crate::ErrorKind;
use crate::FileChange;
use crate::FileContentManager;
use crate::PathContent;

pub struct RepoFileContentManager {
    repo_blobstore: ArcRepoBlobstore,
    bookmarks: ArcBookmarks,
    repo_derived_data: ArcRepoDerivedData,
}

#[async_trait]
impl FileContentManager for RepoFileContentManager {
    async fn get_file_size<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<u64, ErrorKind> {
        Ok(
            filestore::get_metadata(&self.repo_blobstore, ctx, &id.into())
                .await?
                .ok_or(ErrorKind::ContentIdNotFound(id))?
                .total_size,
        )
    }

    async fn get_file_text<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, ErrorKind> {
        filestore::fetch_concat_opt(&self.repo_blobstore, ctx, &id.into())
            .await?
            .ok_or(ErrorKind::ContentIdNotFound(id))
            .map(Option::Some)
    }

    async fn find_content<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: BookmarkName,
        paths: Vec<MPath>,
    ) -> Result<HashMap<MPath, PathContent>, ErrorKind> {
        let changeset_id = self
            .bookmarks
            .get(ctx.clone(), &bookmark)
            .await
            .with_context(|| format!("Error fetching bookmark: {}", bookmark))?
            .ok_or_else(|| format_err!("Bookmark {} does not exist", bookmark))?;

        let master_mf = derive_hg_manifest(
            ctx,
            &self.repo_derived_data,
            &self.repo_blobstore,
            changeset_id,
        )
        .await?;
        master_mf
            .find_entries(ctx.clone(), self.repo_blobstore.clone(), paths)
            .map_ok(|(mb_path, entry)| async move {
                if let Some(path) = mb_path {
                    let content = resolve_content_id(ctx, &self.repo_blobstore, entry).await?;
                    Ok(Some((path, content)))
                } else {
                    Ok(None)
                }
            })
            .try_buffer_unordered(100)
            .try_filter_map(future::ok)
            .try_collect::<HashMap<_, _>>()
            .map_err(ErrorKind::from)
            .await
    }

    async fn file_changes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        new_cs_id: ChangesetId,
        old_cs_id: ChangesetId,
    ) -> Result<Vec<(MPath, FileChange)>, ErrorKind> {
        let new_mf_fut = derive_hg_manifest(
            ctx,
            &self.repo_derived_data,
            &self.repo_blobstore,
            new_cs_id,
        );
        let old_mf_fut = derive_hg_manifest(
            ctx,
            &self.repo_derived_data,
            &self.repo_blobstore,
            old_cs_id,
        );
        let (new_mf, old_mf) = future::try_join(new_mf_fut, old_mf_fut).await?;

        old_mf
            .diff(ctx.clone(), self.repo_blobstore.clone(), new_mf)
            .map_err(ErrorKind::from)
            .map_ok(move |diff| async move {
                match diff {
                    Diff::Added(Some(path), entry) => {
                        match resolve_content_id(ctx, &self.repo_blobstore, entry).await? {
                            PathContent::File(content) => {
                                Ok(Some((path, FileChange::Added(content))))
                            }
                            PathContent::Directory => Ok(None),
                        }
                    }
                    Diff::Changed(Some(path), old_entry, entry) => {
                        let old_content = resolve_content_id(ctx, &self.repo_blobstore, old_entry);
                        let content = resolve_content_id(ctx, &self.repo_blobstore, entry);

                        match future::try_join(old_content, content).await? {
                            (PathContent::File(old_content_id), PathContent::File(content_id)) => {
                                Ok(Some((
                                    path,
                                    FileChange::Changed(old_content_id, content_id),
                                )))
                            }
                            _ => Ok(None),
                        }
                    }
                    Diff::Removed(Some(path), entry) => {
                        if let Entry::Leaf(_) = entry {
                            Ok(Some((path, FileChange::Removed)))
                        } else {
                            Ok(None)
                        }
                    }
                    _ => Ok(None),
                }
            })
            .try_buffer_unordered(100)
            .try_filter_map(future::ok)
            .try_collect()
            .await
    }

    async fn latest_changes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: BookmarkName,
        paths: Vec<MPath>,
    ) -> Result<HashMap<MPath, ChangesetInfo>, ErrorKind> {
        let changeset_id = self
            .bookmarks
            .get(ctx.clone(), &bookmark)
            .await
            .with_context(|| format!("Error fetching bookmark: {}", bookmark))?
            .ok_or_else(|| format_err!("Bookmark {} does not exist", bookmark))?;

        let master_mf = derive_unode_manifest(ctx, &self.repo_derived_data, changeset_id).await?;
        master_mf
            .find_entries(ctx.clone(), self.repo_blobstore.clone(), paths)
            .map_ok(|(mb_path, entry)| async move {
                if let Some(path) = mb_path {
                    let unode = entry
                        .load(ctx, &self.repo_blobstore)
                        .await
                        .with_context(|| format!("Error loading unode entry: {:?}", entry))?;
                    let linknode = match unode {
                        Entry::Leaf(file) => file.linknode().clone(),
                        Entry::Tree(tree) => tree.linknode().clone(),
                    };

                    let cs_info = self
                        .repo_derived_data
                        .derive::<ChangesetInfo>(ctx, linknode)
                        .await
                        .with_context(|| {
                            format!("Error deriving changeset info for bonsai: {}", linknode)
                        })?;

                    Ok(Some((path, cs_info)))
                } else {
                    Ok(None)
                }
            })
            .try_buffer_unordered(100)
            .try_filter_map(future::ok)
            .try_collect::<HashMap<_, _>>()
            .map_err(ErrorKind::from)
            .await
    }
}

impl RepoFileContentManager {
    pub fn new(
        container: &(impl RepoBlobstoreArc + BookmarksArc + RepoDerivedDataArc),
    ) -> RepoFileContentManager {
        let repo_blobstore = container.repo_blobstore_arc();
        let bookmarks = container.bookmarks_arc();
        let repo_derived_data = container.repo_derived_data_arc();

        RepoFileContentManager {
            repo_blobstore,
            bookmarks,
            repo_derived_data,
        }
    }

    pub fn from_parts(
        bookmarks: ArcBookmarks,
        repo_blobstore: ArcRepoBlobstore,
        repo_derived_data: ArcRepoDerivedData,
    ) -> Self {
        RepoFileContentManager {
            repo_blobstore,
            bookmarks,
            repo_derived_data,
        }
    }
}

async fn derive_hg_manifest(
    ctx: &CoreContext,
    repo_derived_data: &RepoDerivedData,
    blobstore: &RepoBlobstore,
    changeset_id: ChangesetId,
) -> Result<HgManifestId, ErrorKind> {
    let hg_changeset_id = repo_derived_data
        .derive::<MappedHgChangesetId>(ctx, changeset_id)
        .await
        .map(|id| id.hg_changeset_id())
        .with_context(|| format!("Error deriving hg changeset for bonsai: {}", changeset_id))?;
    let hg_mf_id = hg_changeset_id
        .load(ctx, blobstore)
        .map_ok(|hg_changeset| hg_changeset.manifestid())
        .await
        .with_context(|| format!("Error loading hg changeset: {}", hg_changeset_id))?;

    Ok(hg_mf_id)
}

async fn derive_unode_manifest(
    ctx: &CoreContext,
    repo_derived_data: &RepoDerivedData,
    changeset_id: ChangesetId,
) -> Result<ManifestUnodeId, ErrorKind> {
    let unode_mf = repo_derived_data
        .derive::<RootUnodeManifestId>(ctx, changeset_id.clone())
        .await
        .with_context(|| format!("Error deriving unode manifest for bonsai: {}", changeset_id))?
        .manifest_unode_id()
        .clone();
    Ok(unode_mf)
}

async fn resolve_content_id(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    entry: Entry<HgManifestId, (FileType, HgFileNodeId)>,
) -> Result<PathContent, ErrorKind> {
    match entry {
        Entry::Tree(_tree) => {
            // there is no content for trees
            Ok(PathContent::Directory)
        }
        Entry::Leaf((_type, file_node_id)) => file_node_id
            .load(ctx, blobstore)
            .map_ok(|file_env| PathContent::File(file_env.content_id()))
            .await
            .with_context(|| format!("Error loading filenode: {}", file_node_id))
            .map_err(ErrorKind::from),
    }
}
