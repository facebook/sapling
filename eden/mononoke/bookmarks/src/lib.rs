/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(never_type)]

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::stream::BoxStream;
use futures::stream::TryStreamExt;
use mononoke_types::ChangesetId;

mod cache;
mod log;
mod subscription;
mod transaction;

pub use bookmarks_types::Bookmark;
pub use bookmarks_types::BookmarkKind;
pub use bookmarks_types::BookmarkName;
pub use bookmarks_types::BookmarkPagination;
pub use bookmarks_types::BookmarkPrefix;
pub use bookmarks_types::BookmarkPrefixRange;
pub use bookmarks_types::Freshness;
pub use cache::CachedBookmarks;
pub use log::ArcBookmarkUpdateLog;
pub use log::BookmarkUpdateLog;
pub use log::BookmarkUpdateLogArc;
pub use log::BookmarkUpdateLogEntry;
pub use log::BookmarkUpdateLogRef;
pub use log::BookmarkUpdateReason;
pub use subscription::BookmarksSubscription;
pub use transaction::BookmarkTransaction;
pub use transaction::BookmarkTransactionError;
pub use transaction::BookmarkTransactionHook;

#[facet::facet]
#[async_trait]
pub trait Bookmarks: Send + Sync + 'static {
    /// Get the current value of a bookmark.
    ///
    /// Returns `Some(ChangesetId)` if the bookmark exists, or `None` if doesn't
    fn get(
        &self,
        ctx: CoreContext,
        name: &BookmarkName,
    ) -> BoxFuture<'static, Result<Option<ChangesetId>>>;

    /// List bookmarks that match certain parameters.
    ///
    /// `prefix` requires that bookmark names begin with a certain prefix.
    ///
    /// `kinds` requires that the bookmark is of a certain kind.
    ///
    /// `pagination` limits bookmarks to those lexicographically after the
    /// named bookmark for pagination purposes.
    ///
    /// `limit` limits the total number of bookmarks returned.
    ///
    /// Bookmarks are returned in lexicographic order.  If a request
    /// hits the limit, then a subsequent request with `pagination`
    /// set to `BookmarkPagination::After(name)` will allow listing
    /// of the remaining bookmarks.
    fn list(
        &self,
        ctx: CoreContext,
        freshness: Freshness,
        prefix: &BookmarkPrefix,
        kinds: &[BookmarkKind],
        pagination: &BookmarkPagination,
        limit: u64,
    ) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>>;

    /// Create a transaction to modify bookmarks.
    fn create_transaction(&self, ctx: CoreContext) -> Box<dyn BookmarkTransaction>;

    /// Create a subscription to efficiently observe changes to publishing & pull default
    /// bookmarks.
    async fn create_subscription(
        &self,
        ctx: &CoreContext,
        freshness: Freshness,
    ) -> Result<Box<dyn BookmarksSubscription>>;

    /// Drop any caches held by this instance of Bookmarks.
    fn drop_caches(&self) {
        // No-op by default.
    }
}

/// Construct a heads fetcher (function that returns all the heads in the
/// repo) that uses the publishing bookmarks as all heads.
pub fn bookmark_heads_fetcher(
    bookmarks: ArcBookmarks,
) -> Arc<dyn Fn(&CoreContext) -> BoxFuture<'static, Result<Vec<ChangesetId>>> + Send + Sync> {
    Arc::new({
        move |ctx: &CoreContext| {
            bookmarks
                .list(
                    ctx.clone(),
                    Freshness::MaybeStale,
                    &BookmarkPrefix::empty(),
                    BookmarkKind::ALL_PUBLISHING,
                    &BookmarkPagination::FromStart,
                    std::u64::MAX,
                )
                .map_ok(|(_, cs_id)| cs_id)
                .try_collect()
                .boxed()
        }
    })
}
