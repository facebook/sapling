/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use bookmarks_types::BookmarkKey;
use bookmarks_types::BookmarkKind;
use bookmarks_types::BookmarkPagination;
use bookmarks_types::BookmarkPrefix;
use context::CoreContext;
use mononoke_types::ChangesetId;

#[async_trait]
#[facet::facet]
pub trait BookmarksCache: Send + Sync {
    async fn get(
        &self,
        ctx: &CoreContext,
        bookmark: &BookmarkKey,
    ) -> Result<Option<ChangesetId>, Error>;

    async fn list(
        &self,
        ctx: &CoreContext,
        prefix: &BookmarkPrefix,
        pagination: &BookmarkPagination,
        limit: Option<u64>,
    ) -> Result<Vec<(BookmarkKey, (ChangesetId, BookmarkKind))>, Error>;

    /// Awaits the completion of any ongoing update.
    async fn sync(&self, ctx: &CoreContext);
}

/// The warmers that are configured in the bookmarks_cache are tagged according
/// to their use. Some of them are used for Hg, some for Git, and some for both.
/// This enum allows selecting the warmup requirements for the requested bookmark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WarmerRequirement {
    /// All the Hg Warmers must be ready
    HgOnly,

    /// All the Git Warmers must be ready
    GitOnly,

    /// All the configured Warmers must be ready
    AllKinds,
}

#[async_trait]
#[facet::facet]
pub trait ScopedBookmarksCache: Send + Sync {
    /// Get the ChangesetId pointed to by the given bookmark, scoped to the
    /// specified WarmerRequirement. The requested changeset is guaranteed to
    /// have the necessary derived data available.
    async fn get(
        &self,
        ctx: &CoreContext,
        bookmark: &BookmarkKey,
        scope: WarmerRequirement,
    ) -> Result<Option<ChangesetId>, Error>;

    /// List the bookmarks starting with the specified prefix and that are warm
    /// according to the given requirements.
    async fn list(
        &self,
        ctx: &CoreContext,
        prefix: &BookmarkPrefix,
        pagination: &BookmarkPagination,
        limit: Option<u64>,
        scope: WarmerRequirement,
    ) -> Result<Vec<(BookmarkKey, (ChangesetId, BookmarkKind))>, Error>;

    /// Awaits the completion of any ongoing update.
    async fn sync(&self, ctx: &CoreContext);
}

#[async_trait]
impl<T: ScopedBookmarksCache> BookmarksCache for T {
    async fn get(
        &self,
        ctx: &CoreContext,
        bookmark: &BookmarkKey,
    ) -> Result<Option<ChangesetId>, Error> {
        self.get(ctx, bookmark, WarmerRequirement::AllKinds).await
    }

    async fn list(
        &self,
        ctx: &CoreContext,
        prefix: &BookmarkPrefix,
        pagination: &BookmarkPagination,
        limit: Option<u64>,
    ) -> Result<Vec<(BookmarkKey, (ChangesetId, BookmarkKind))>, Error> {
        self.list(ctx, prefix, pagination, limit, WarmerRequirement::AllKinds)
            .await
    }

    async fn sync(&self, ctx: &CoreContext) {
        self.sync(ctx).await;
    }
}
