/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod memory;
pub mod text_only;

use std::collections::HashMap;

use async_trait::async_trait;
use bookmarks_types::BookmarkKey;
use bytes::Bytes;
use changeset_info::ChangesetInfo;
use context::CoreContext;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadataV2;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;

use crate::errors::HookStateProviderError;

/// Enum describing the state of a bookmark for which hooks are being run.
pub enum BookmarkState {
    /// The bookmark is new and is being created by the current push
    New,
    /// The bookmark is existing and is being moved by the current push
    Existing(ChangesetId),
    // No Deleted state because hooks are not run on deleted bookmarks
}

impl BookmarkState {
    pub fn is_new(&self) -> bool {
        if let BookmarkState::New = *self {
            return true;
        }
        false
    }
}

/// Enum describing the type of a tag for which hooks are being run.
pub enum TagType {
    /// The bookmark is not a tag at all
    NotATag,
    /// The bookmark is a simple tag with no object associated with it
    LightweightTag,
    /// The bookmark is an annotated tag with an associated object with GitSha1 hash
    AnnotatedTag(GitSha1),
}

/// Trait implemented by providers of content for hooks to analyze.
#[async_trait]
pub trait HookStateProvider: Send + Sync {
    /// The size of a file with a particular content id.
    async fn get_file_metadata<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<ContentMetadataV2, HookStateProviderError>;

    /// The text of a file with a particular content id.  If the content is
    /// not appropriate to analyze (e.g. because it is too large), then the
    /// provider may return `None`.
    async fn get_file_text<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, HookStateProviderError>;

    /// The state of a bookmark at the time the push is being run. Note that this
    /// is best effort since the bookmark can move as a result of another push
    /// happening concurrently
    async fn get_bookmark_state<'a, 'b>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: &'b BookmarkKey,
    ) -> Result<BookmarkState, HookStateProviderError>;

    /// The type of a tag at the time the push is being run. Useful for determining
    /// if the bookmark being pushed is a tag or not and if its a tag, if its a simple
    /// or annotated.
    async fn get_tag_type<'a, 'b>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: &'b BookmarkKey,
    ) -> Result<TagType, HookStateProviderError>;

    /// If the repo for which the hook is being run is a Git repo, return the corresponding
    /// Git commit hash for the given Bonsai commit.
    async fn get_git_commit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bonsai_commit_id: ChangesetId,
    ) -> Result<Option<GitSha1>, HookStateProviderError>;

    /// Find the content of a set of files at a particular bookmark.
    async fn find_content<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: BookmarkKey,
        paths: Vec<NonRootMPath>,
    ) -> Result<HashMap<NonRootMPath, PathContent>, HookStateProviderError>;

    /// Find all changes between two changeset ids.
    async fn file_changes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        new_cs_id: ChangesetId,
        old_cs_id: ChangesetId,
    ) -> Result<Vec<(NonRootMPath, FileChangeType)>, HookStateProviderError>;

    /// Find the latest changesets that affected a set of paths at a particular bookmark.
    async fn latest_changes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: BookmarkKey,
        paths: Vec<NonRootMPath>,
    ) -> Result<HashMap<NonRootMPath, ChangesetInfo>, HookStateProviderError>;

    /// Find the count of child entries in a set of paths
    async fn directory_sizes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        changeset_id: ChangesetId,
        paths: Vec<MPath>,
    ) -> Result<HashMap<MPath, u64>, HookStateProviderError>;
}

#[derive(Clone, Debug)]
pub enum PathContent {
    Directory,
    File(ContentId),
}

#[derive(Clone, Debug)]
pub enum FileChangeType {
    Added(ContentId),
    Changed(ContentId, ContentId),
    Removed,
}
