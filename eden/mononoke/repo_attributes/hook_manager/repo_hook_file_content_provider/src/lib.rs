/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use anyhow::format_err;
use anyhow::Context as _;
use async_trait::async_trait;
use blobstore::Loadable;
use bonsai_git_mapping::ArcBonsaiGitMapping;
use bonsai_git_mapping::BonsaiGitMappingArc;
use bonsai_tag_mapping::ArcBonsaiTagMapping;
use bonsai_tag_mapping::BonsaiTagMappingArc;
use bonsai_tag_mapping::Freshness;
use bookmarks::ArcBookmarks;
use bookmarks::BookmarkKey;
use bookmarks::BookmarksArc;
use bytes::Bytes;
use changeset_info::ChangesetInfo;
use context::CoreContext;
use fsnodes::RootFsnodeId;
use futures::future;
use futures::stream::TryStreamExt;
use futures_util::future::TryFutureExt;
use hook_manager::provider::BookmarkState;
use hook_manager::provider::TagType;
use hook_manager::FileChangeType;
use hook_manager::HookStateProvider;
use hook_manager::HookStateProviderError;
use hook_manager::PathContent;
use manifest::Diff;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadataV2;
use mononoke_types::FsnodeId;
use mononoke_types::MPath;
use mononoke_types::ManifestUnodeId;
use mononoke_types::NonRootMPath;
use repo_blobstore::ArcRepoBlobstore;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::ArcRepoDerivedData;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataArc;
use skeleton_manifest::RootSkeletonManifestId;
use unodes::RootUnodeManifestId;

pub struct RepoHookStateProvider {
    repo_blobstore: ArcRepoBlobstore,
    bookmarks: ArcBookmarks,
    repo_derived_data: ArcRepoDerivedData,
    bonsai_tag_mapping: ArcBonsaiTagMapping,
    bonsai_git_mapping: ArcBonsaiGitMapping,
}

#[async_trait]
impl HookStateProvider for RepoHookStateProvider {
    async fn get_file_metadata<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<ContentMetadataV2, HookStateProviderError> {
        Ok(
            filestore::get_metadata(&self.repo_blobstore, ctx, &id.into())
                .await?
                .ok_or(HookStateProviderError::ContentIdNotFound(id))?,
        )
    }

    async fn get_file_text<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, HookStateProviderError> {
        filestore::fetch_concat_opt(&self.repo_blobstore, ctx, &id.into())
            .await?
            .ok_or(HookStateProviderError::ContentIdNotFound(id))
            .map(Option::Some)
    }

    async fn find_content<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: BookmarkKey,
        paths: Vec<NonRootMPath>,
    ) -> Result<HashMap<NonRootMPath, PathContent>, HookStateProviderError> {
        let changeset_id = self
            .bookmarks
            .get(ctx.clone(), &bookmark)
            .await
            .with_context(|| format!("Error fetching bookmark: {}", bookmark))?
            .ok_or_else(|| format_err!("Bookmark {} does not exist", bookmark))?;

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
            .map_err(HookStateProviderError::from)
            .await
    }

    async fn file_changes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        new_cs_id: ChangesetId,
        old_cs_id: ChangesetId,
    ) -> Result<Vec<(NonRootMPath, FileChangeType)>, HookStateProviderError> {
        let new_mf_fut = derive_fsnode(ctx, &self.repo_derived_data, new_cs_id);
        let old_mf_fut = derive_fsnode(ctx, &self.repo_derived_data, old_cs_id);

        let (new_mf, old_mf) = future::try_join(new_mf_fut, old_mf_fut).await?;

        old_mf
            .diff(ctx.clone(), self.repo_blobstore.clone(), new_mf)
            .map_err(HookStateProviderError::from)
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

    async fn latest_changes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: BookmarkKey,
        paths: Vec<NonRootMPath>,
    ) -> Result<HashMap<NonRootMPath, ChangesetInfo>, HookStateProviderError> {
        let changeset_id = self
            .bookmarks
            .get(ctx.clone(), &bookmark)
            .await
            .with_context(|| format!("Error fetching bookmark: {}", bookmark))?
            .ok_or_else(|| format_err!("Bookmark {} does not exist", bookmark))?;

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
            .map_err(HookStateProviderError::from)
            .await
    }

    async fn directory_sizes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        changeset_id: ChangesetId,
        paths: Vec<MPath>,
    ) -> Result<HashMap<MPath, u64>, HookStateProviderError> {
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
            .map_err(HookStateProviderError::from)
    }

    async fn get_bookmark_state<'a, 'b>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: &'b BookmarkKey,
    ) -> Result<BookmarkState, HookStateProviderError> {
        let maybe_bookmark_val = self
            .bookmarks
            .get(ctx.clone(), bookmark)
            .await
            .with_context(|| format!("Error fetching bookmark: {}", bookmark))?;
        if let Some(cs_id) = maybe_bookmark_val {
            Ok(BookmarkState::Existing(cs_id))
        } else {
            Ok(BookmarkState::New)
        }
    }

    async fn get_tag_type<'a, 'b>(
        &'a self,
        _ctx: &'a CoreContext,
        bookmark: &'b BookmarkKey,
    ) -> Result<TagType, HookStateProviderError> {
        if !bookmark.is_tag() {
            return Ok(TagType::NotATag);
        }
        match self
            .bonsai_tag_mapping
            .get_entry_by_tag_name(bookmark.to_string(), Freshness::Latest)
            .await?
        {
            Some(entry) => Ok(TagType::AnnotatedTag(entry.tag_hash)),
            None => Ok(TagType::LightweightTag),
        }
    }

    async fn get_git_commit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bonsai_commit_id: ChangesetId,
    ) -> Result<Option<GitSha1>, HookStateProviderError> {
        let maybe_git_sha1 = self
            .bonsai_git_mapping
            .get_git_sha1_from_bonsai(ctx, bonsai_commit_id)
            .await?;
        Ok(maybe_git_sha1)
    }
}

impl RepoHookStateProvider {
    pub fn new(
        container: &(
             impl RepoBlobstoreArc
             + BookmarksArc
             + RepoDerivedDataArc
             + BonsaiTagMappingArc
             + BonsaiGitMappingArc
         ),
    ) -> RepoHookStateProvider {
        let repo_blobstore = container.repo_blobstore_arc();
        let bookmarks = container.bookmarks_arc();
        let repo_derived_data = container.repo_derived_data_arc();
        let bonsai_tag_mapping = container.bonsai_tag_mapping_arc();
        let bonsai_git_mapping = container.bonsai_git_mapping_arc();

        RepoHookStateProvider {
            repo_blobstore,
            bookmarks,
            repo_derived_data,
            bonsai_tag_mapping,
            bonsai_git_mapping,
        }
    }

    pub fn from_parts(
        bookmarks: ArcBookmarks,
        repo_blobstore: ArcRepoBlobstore,
        repo_derived_data: ArcRepoDerivedData,
        bonsai_tag_mapping: ArcBonsaiTagMapping,
        bonsai_git_mapping: ArcBonsaiGitMapping,
    ) -> Self {
        RepoHookStateProvider {
            repo_blobstore,
            bookmarks,
            repo_derived_data,
            bonsai_tag_mapping,
            bonsai_git_mapping,
        }
    }
}

async fn derive_fsnode(
    ctx: &CoreContext,
    repo_derived_data: &RepoDerivedData,
    changeset_id: ChangesetId,
) -> Result<FsnodeId, HookStateProviderError> {
    let fsnode_id = repo_derived_data
        .derive::<RootFsnodeId>(ctx, changeset_id.clone())
        .await
        .with_context(|| {
            format!(
                "Error deriving fsnode manifest for bonsai: {}",
                changeset_id
            )
        })?
        .into_fsnode_id()
        .clone();

    Ok(fsnode_id)
}

async fn derive_unode_manifest(
    ctx: &CoreContext,
    repo_derived_data: &RepoDerivedData,
    changeset_id: ChangesetId,
) -> Result<ManifestUnodeId, HookStateProviderError> {
    let unode_mf = repo_derived_data
        .derive::<RootUnodeManifestId>(ctx, changeset_id.clone())
        .await
        .with_context(|| format!("Error deriving unode manifest for bonsai: {}", changeset_id))?
        .manifest_unode_id()
        .clone();
    Ok(unode_mf)
}
