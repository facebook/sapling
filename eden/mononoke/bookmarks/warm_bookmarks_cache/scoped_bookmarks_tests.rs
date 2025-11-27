/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests for scoped bookmark tracking functionality.
//!
//! These tests verify that the warm bookmarks cache correctly tracks different
//! pointers for different warmer requirements (HgOnly, GitOnly, AllKinds).

use std::sync::Arc;

use anyhow::Result;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use context::CoreContext;
use fbinit::FacebookInit;
use fixtures::Linear;
use fixtures::TestRepoFixture;
use mononoke_macros::mononoke;
use tests_utils::bookmark;
use tests_utils::resolve_cs_id;

use crate::BookmarksCache;
use crate::InitMode;
use crate::init_bookmarks;
use crate::test_helpers::*;
use crate::tests::Repo;

#[mononoke::fbinit_test]
async fn test_bookmark_visibility(fb: FacebookInit) -> Result<()> {
    // Test that bookmarks appear as soon as ANY scope is ready (not just AllKinds).
    // Bookmarks become visible with partial warming, allowing scoped API users to
    // see progress before all warmers complete.

    let repo: Repo = Linear::get_repo(fb).await;
    let ctx = CoreContext::test_mock(fb);

    let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;

    // Step 1: Derive only Hg data (and common dependencies), but not Git
    derive_data(&ctx, &repo, master_cs_id, true, false, true).await?;

    let warmers = setup_warmers(&ctx, &repo, WarmerConfig::all_warmers());

    // Step 2: Initialize bookmarks - should include master since HgOnly is derived
    let sub = repo
        .bookmarks()
        .create_subscription(&ctx, Freshness::MostRecent)
        .await?;

    let bookmarks = init_bookmarks(
        &ctx,
        &*sub,
        repo.bookmarks(),
        repo.bookmark_update_log(),
        &warmers,
        InitMode::Rewind,
    )
    .await?;

    let master_bookmark = BookmarkKey::new("master")?;
    let state = bookmarks
        .get(&master_bookmark)
        .expect("Bookmark should appear when HgOnly is derived (even without Git)");

    // Verify HgOnly is set, but AllKinds is not (since Git is missing)
    assert_bookmark_pointers(
        state,
        Some(master_cs_id),
        None, // Git not derived yet
        None, // AllKinds requires both Hg and Git
        "After deriving only Hg",
    );

    // Step 3: Now derive Git data to make AllKinds ready
    derive_data(&ctx, &repo, master_cs_id, false, true, false).await?;

    let bookmarks_after_git = init_bookmarks(
        &ctx,
        &*sub,
        repo.bookmarks(),
        repo.bookmark_update_log(),
        &warmers,
        InitMode::Rewind,
    )
    .await?;

    // Step 4: Verify bookmark now has all scopes tracked
    let state_after = bookmarks_after_git
        .get(&master_bookmark)
        .expect("Bookmark should still be present after deriving Git");

    assert_bookmark_pointers(
        state_after,
        Some(master_cs_id),
        Some(master_cs_id),
        Some(master_cs_id),
        "After deriving both Hg and Git",
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_scoped_pointers_track_independently(fb: FacebookInit) -> Result<()> {
    // Test that different scopes can track different pointers based on
    // what data has been derived for each commit.

    let repo: Repo = Linear::get_repo(fb).await;
    let ctx = CoreContext::test_mock(fb);

    let warmers = setup_warmers(&ctx, &repo, WarmerConfig::all_warmers());
    let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;

    // Build commit chain with different derivation patterns
    let mut builder = CommitChainBuilder::new(&ctx, &repo, master_cs_id);

    // commit1: Both Hg and Git derived, with unodes
    let commit1 = builder.add_commit("file1", true, true, true).await?;
    bookmark(&ctx, &repo, "test_bookmark")
        .create_publishing(commit1)
        .await?;

    // commit2: Only Hg derived explicitly, with unodes
    let commit2 = builder.add_commit("file2", true, false, true).await?;
    bookmark(&ctx, &repo, "test_bookmark")
        .set_to(commit2)
        .await?;

    // commit3: Only Git derived (will transitively derive Git on commit2), with unodes
    let commit3 = builder.add_commit("file3", false, true, true).await?;
    bookmark(&ctx, &repo, "test_bookmark")
        .set_to(commit3)
        .await?;

    // commit4: Nothing derived
    let commit4 = builder.add_commit("file4", false, false, false).await?;
    bookmark(&ctx, &repo, "test_bookmark")
        .set_to(commit4)
        .await?;

    // Initialize bookmarks with rewind mode
    let sub = repo
        .bookmarks()
        .create_subscription(&ctx, Freshness::MostRecent)
        .await?;

    let bookmarks = init_bookmarks(
        &ctx,
        &*sub,
        repo.bookmarks(),
        repo.bookmark_update_log(),
        &warmers,
        InitMode::Rewind,
    )
    .await?;

    // Verify internal state tracking
    let state = bookmarks
        .get(&BookmarkKey::new("test_bookmark")?)
        .expect("Bookmark should exist");

    // Due to transitive derivation when deriving Git on commit3,
    // commit2 also gets Git derived, making it have both Hg and Git
    assert_bookmark_pointers(
        state,
        Some(commit2), // HgOnly: latest with Hg
        Some(commit3), // GitOnly: latest with Git
        Some(commit2), // AllKinds: commit2 has both due to transitive derivation
        "Scoped pointer tracking",
    );

    // Test via ScopedBookmarksCache API
    let warmers_for_cache = setup_warmers(&ctx, &repo, WarmerConfig::all_warmers());
    let cache = init_cache_with_warmers(
        &ctx,
        &repo,
        Arc::try_unwrap(warmers_for_cache).ok().unwrap(),
        InitMode::Rewind,
    )
    .await?;

    assert_scoped_get(
        &cache,
        &ctx,
        &BookmarkKey::new("test_bookmark")?,
        Some(commit2),
        Some(commit3),
        Some(commit2),
        "ScopedBookmarksCache API",
    )
    .await?;

    // Verify regular BookmarksCache::get returns the AllKinds pointer
    let regular_result =
        BookmarksCache::get(&cache, &ctx, &BookmarkKey::new("test_bookmark")?).await?;
    assert_eq!(
        regular_result,
        Some(commit2),
        "Regular BookmarksCache should return AllKinds pointer"
    );

    Ok(())
}
