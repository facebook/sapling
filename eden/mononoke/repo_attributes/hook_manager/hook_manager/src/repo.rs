/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use blobstore::Loadable;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_tag_mapping::BonsaiTagMapping;
use bonsai_tag_mapping::Freshness;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::Bookmarks;
use bytes::Bytes;
use changeset_info::ChangesetInfo;
use commit_graph::CommitGraph;
use context::CoreContext;
use fsnodes::RootFsnodeId;
use futures::future;
use futures::stream::TryStreamExt;
use manifest::Diff;
use manifest::Entry;
use manifest::ManifestOps;
use metaconfig_types::RepoConfig;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadataV2;
use mononoke_types::FsnodeId;
use mononoke_types::MPath;
use mononoke_types::ManifestUnodeId;
use mononoke_types::NonRootMPath;
use mononoke_types::hash::GitSha1;
use repo_blobstore::RepoBlobstore;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use skeleton_manifest::RootSkeletonManifestId;
use unodes::RootUnodeManifestId;

use crate::BookmarkState;
use crate::FileChangeType;
use crate::PathContent;
use crate::TagType;

/// Repo available to hooks.
#[facet::container]
#[derive(Clone)]
pub struct HookRepo {
    #[facet]
    pub repo_identity: RepoIdentity,

    #[facet]
    pub repo_config: RepoConfig,

    #[facet]
    pub repo_blobstore: RepoBlobstore,

    #[facet]
    pub bookmarks: dyn Bookmarks,

    #[facet]
    pub repo_derived_data: RepoDerivedData,

    #[facet]
    pub bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    pub bonsai_tag_mapping: dyn BonsaiTagMapping,

    #[facet]
    pub repo_cross_repo: RepoCrossRepo,

    #[facet]
    pub commit_graph: CommitGraph,
}

impl HookRepo {
    pub async fn get_file_metadata<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<ContentMetadataV2> {
        filestore::get_metadata(&self.repo_blobstore, ctx, &id.into())
            .await?
            .ok_or_else(|| anyhow!("Content with id '{id}' not found"))
    }

    pub async fn get_bonsai_changeset<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ChangesetId,
    ) -> Result<BonsaiChangeset> {
        Ok(id.load(ctx, &self.repo_blobstore).await?)
    }

    pub async fn get_file_text<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>> {
        let file_bytes = self.get_file_bytes(ctx, id).await?;

        // Filter out files with null bytes
        let file_bytes = file_bytes.filter(|b| !b.contains(&0));

        Ok(file_bytes)
    }

    pub async fn get_file_bytes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>> {
        // Don't fetch content if we know the object is too large
        let size = self.get_file_metadata(ctx, id).await?.total_size;
        if size > self.repo_config.hook_max_file_size {
            return Ok(None);
        }

        let file_bytes = filestore::fetch_concat_opt(&self.repo_blobstore, ctx, &id.into())
            .await?
            .ok_or_else(|| anyhow!("Content with id '{id}' not found"))?;

        Ok(Some(file_bytes))
    }

    pub async fn find_content<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: BookmarkKey,
        paths: Vec<NonRootMPath>,
    ) -> Result<HashMap<NonRootMPath, PathContent>> {
        let changeset_id = self
            .bookmarks
            .get(ctx.clone(), &bookmark, bookmarks::Freshness::MostRecent)
            .await
            .with_context(|| format!("Error fetching bookmark: {}", bookmark))?
            .ok_or_else(|| anyhow!("Bookmark {} does not exist", bookmark))?;

        self.find_content_by_changeset_id(ctx, changeset_id, paths)
            .await
    }

    pub async fn find_content_by_changeset_id<'a>(
        &'a self,
        ctx: &'a CoreContext,
        changeset_id: ChangesetId,
        paths: Vec<NonRootMPath>,
    ) -> Result<HashMap<NonRootMPath, PathContent>> {
        let fsnode_id = derive_fsnode(ctx, &self.repo_derived_data, changeset_id).await?;

        fsnode_id
            .find_entries(ctx.clone(), self.repo_blobstore.clone(), paths)
            .map_ok(|(mb_path, entry)| async move {
                if let Some(path) = Option::<NonRootMPath>::from(mb_path) {
                    match entry {
                        Entry::Tree(_) => Ok(Some((path, PathContent::Directory))),
                        Entry::Leaf(file) => {
                            let content_id = file.content_id();
                            Ok(Some((path, PathContent::File(*content_id))))
                        }
                    }
                } else {
                    Ok(None)
                }
            })
            .try_buffer_unordered(100)
            .try_filter_map(future::ok)
            .try_collect::<HashMap<_, _>>()
            .await
    }

    pub async fn file_changes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        new_cs_id: ChangesetId,
        old_cs_id: ChangesetId,
    ) -> Result<Vec<(NonRootMPath, FileChangeType)>> {
        let new_mf_fut = derive_fsnode(ctx, &self.repo_derived_data, new_cs_id);
        let old_mf_fut = derive_fsnode(ctx, &self.repo_derived_data, old_cs_id);

        let (new_mf, old_mf) = future::try_join(new_mf_fut, old_mf_fut).await?;

        old_mf
            .diff(ctx.clone(), self.repo_blobstore.clone(), new_mf)
            .map_ok(move |diff| async move {
                match diff {
                    Diff::Added(path, entry) => match Option::<NonRootMPath>::from(path) {
                        Some(path) => match entry {
                            Entry::Tree(_) => Ok(None),
                            Entry::Leaf(c) => {
                                Ok(Some((path, FileChangeType::Added(*c.content_id()))))
                            }
                        },
                        None => Ok(None),
                    },
                    Diff::Changed(path, old_entry, entry) if !path.is_root() => {
                        match Option::<NonRootMPath>::from(path) {
                            Some(path) => match (old_entry, entry) {
                                (Entry::Leaf(old_c), Entry::Leaf(c)) => Ok(Some((
                                    path,
                                    FileChangeType::Changed(*old_c.content_id(), *c.content_id()),
                                ))),
                                _ => Ok(None),
                            },
                            None => Ok(None),
                        }
                    }
                    Diff::Removed(path, entry) => match Option::<NonRootMPath>::from(path) {
                        Some(path) => {
                            if let Entry::Leaf(_) = entry {
                                Ok(Some((path, FileChangeType::Removed)))
                            } else {
                                Ok(None)
                            }
                        }
                        None => Ok(None),
                    },
                    _ => Ok(None),
                }
            })
            .try_buffer_unordered(100)
            .try_filter_map(future::ok)
            .try_collect()
            .await
    }

    pub async fn latest_changes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: BookmarkKey,
        paths: Vec<NonRootMPath>,
    ) -> Result<HashMap<NonRootMPath, ChangesetInfo>> {
        let changeset_id = self
            .bookmarks
            .get(ctx.clone(), &bookmark, bookmarks::Freshness::MostRecent)
            .await
            .with_context(|| format!("Error fetching bookmark: {}", bookmark))?
            .ok_or_else(|| anyhow!("Bookmark {} does not exist", bookmark))?;

        let master_mf = derive_unode_manifest(ctx, &self.repo_derived_data, changeset_id).await?;
        master_mf
            .find_entries(ctx.clone(), self.repo_blobstore.clone(), paths)
            .map_ok(|(path, entry)| async move {
                if let Some(path) = Option::<NonRootMPath>::from(path) {
                    let linknode = match entry {
                        Entry::Leaf(file_id) => {
                            let file = file_id.load(ctx, &self.repo_blobstore).await?;
                            file.linknode().clone()
                        }
                        Entry::Tree(tree_id) => {
                            let tree = tree_id.load(ctx, &self.repo_blobstore).await?;
                            tree.linknode().clone()
                        }
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
            .await
    }

    pub async fn directory_sizes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        changeset_id: ChangesetId,
        paths: Vec<MPath>,
    ) -> Result<HashMap<MPath, u64>> {
        let sk_mf = self
            .repo_derived_data
            .derive::<RootSkeletonManifestId>(ctx, changeset_id)
            .await
            .with_context(|| format!("Error deriving skeleton manifest for {}", changeset_id))?
            .skeleton_manifest_id()
            .clone();
        sk_mf
            .find_entries(ctx.clone(), self.repo_blobstore.clone(), paths)
            .try_filter_map(|(path, entry)| async move {
                match entry {
                    Entry::Tree(tree_id) => {
                        let tree = tree_id.load(ctx, &self.repo_blobstore).await?;
                        let summary = tree.summary();
                        Ok(Some((
                            path,
                            summary.child_files_count + summary.child_dirs_count,
                        )))
                    }
                    _ => Ok(None),
                }
            })
            .try_collect()
            .await
    }

    pub async fn get_bookmark_state<'a, 'b>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: &'b BookmarkKey,
    ) -> Result<BookmarkState> {
        let maybe_bookmark_val = self
            .bookmarks
            .get(ctx.clone(), bookmark, bookmarks::Freshness::MostRecent)
            .await
            .with_context(|| format!("Error fetching bookmark: {}", bookmark))?;
        if let Some(cs_id) = maybe_bookmark_val {
            Ok(BookmarkState::Existing(cs_id))
        } else {
            Ok(BookmarkState::New)
        }
    }

    pub async fn bookmark_exists_with_prefix<'a, 'b>(
        &'a self,
        ctx: CoreContext,
        prefix: &'b BookmarkPrefix,
    ) -> Result<bool> {
        let bookmark_with_prefix_count = self
            .bookmarks
            .list(
                ctx,
                bookmarks::Freshness::MaybeStale,
                prefix,
                BookmarkCategory::ALL,
                BookmarkKind::ALL_PUBLISHING,
                &BookmarkPagination::FromStart,
                1,
            )
            .try_collect::<Vec<_>>()
            .await
            .with_context(|| format!("Error fetching bookmarks with prefix {prefix}"))?
            .len();

        Ok(bookmark_with_prefix_count > 0)
    }

    pub async fn get_tag_type<'a, 'b>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: &'b BookmarkKey,
    ) -> Result<TagType> {
        if !bookmark.is_tag() {
            return Ok(TagType::NotATag);
        }
        match self
            .bonsai_tag_mapping
            .get_entry_by_tag_name(ctx, bookmark.to_string(), Freshness::Latest)
            .await?
        {
            Some(entry) => Ok(TagType::AnnotatedTag(entry.tag_hash)),
            None => Ok(TagType::LightweightTag),
        }
    }

    pub async fn get_git_commit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bonsai_commit_id: ChangesetId,
    ) -> Result<Option<GitSha1>> {
        let maybe_git_sha1 = self
            .bonsai_git_mapping
            .get_git_sha1_from_bonsai(ctx, bonsai_commit_id)
            .await?;
        Ok(maybe_git_sha1)
    }
}

async fn derive_fsnode(
    ctx: &CoreContext,
    repo_derived_data: &RepoDerivedData,
    changeset_id: ChangesetId,
) -> Result<FsnodeId> {
    let fsnode_id = repo_derived_data
        .derive::<RootFsnodeId>(ctx, changeset_id.clone())
        .await
        .with_context(|| {
            format!(
                "Error deriving fsnode manifest for bonsai: {}",
                changeset_id
            )
        })?
        .into_fsnode_id();

    Ok(fsnode_id)
}

async fn derive_unode_manifest(
    ctx: &CoreContext,
    repo_derived_data: &RepoDerivedData,
    changeset_id: ChangesetId,
) -> Result<ManifestUnodeId> {
    let unode_mf = repo_derived_data
        .derive::<RootUnodeManifestId>(ctx, changeset_id.clone())
        .await
        .with_context(|| format!("Error deriving unode manifest for bonsai: {}", changeset_id))?
        .manifest_unode_id()
        .clone();
    Ok(unode_mf)
}
