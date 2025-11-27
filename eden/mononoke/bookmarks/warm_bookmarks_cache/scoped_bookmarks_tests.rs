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
use tests_utils::resolve_cs_id;

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
