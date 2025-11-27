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

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use bookmarks_cache::ScopedBookmarksCache;
use bookmarks_cache::WarmerRequirement;
use context::CoreContext;
use fbinit::FacebookInit;
use fixtures::Linear;
use fixtures::TestRepoFixture;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use tests_utils::CreateCommitContext;
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

#[mononoke::fbinit_test]
async fn test_complex_warmer_dependencies(fb: FacebookInit) -> Result<()> {
    // Test with warmers that have dependencies (Unodes requires both Hg and Git)

    let repo: Repo = Linear::get_repo(fb).await;
    let ctx = CoreContext::test_mock(fb);

    let warmers = setup_warmers(&ctx, &repo, WarmerConfig::all_warmers());
    let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;

    let mut builder = CommitChainBuilder::new(&ctx, &repo, master_cs_id);

    // commit1: Everything derived
    let commit1 = builder.add_commit("file1", true, true, true).await?;
    bookmark(&ctx, &repo, "complex_bookmark")
        .create_publishing(commit1)
        .await?;

    // commit2: Hg
    let commit2 = builder.add_commit("file2", true, false, true).await?;
    bookmark(&ctx, &repo, "complex_bookmark")
        .set_to(commit2)
        .await?;

    // commit3: Git
    let commit3 = builder.add_commit("file3", false, true, true).await?;
    bookmark(&ctx, &repo, "complex_bookmark")
        .set_to(commit3)
        .await?;

    // commit4: Only Hg and Git specific, but none complete
    let commit4 = builder.add_commit("file4", true, true, false).await?;
    bookmark(&ctx, &repo, "complex_bookmark")
        .set_to(commit4)
        .await?;

    let cache = init_cache_with_warmers(
        &ctx,
        &repo,
        Arc::try_unwrap(warmers).ok().unwrap(),
        InitMode::Rewind,
    )
    .await?;

    assert_scoped_get(
        &cache,
        &ctx,
        &BookmarkKey::new("complex_bookmark")?,
        Some(commit3), // HgOnly: through transitive derivation, Hg was derived for 3, when we derived 4
        Some(commit3), // GitOnly: All requirements present at 3
        Some(commit3), // AllKinds: transitive derivation gives everything at 3
        "Complex warmer dependencies",
    )
    .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_scoped_list_filtering(fb: FacebookInit) -> Result<()> {
    // Test that list() API correctly filters bookmarks by scope

    let repo: Repo = Linear::get_repo(fb).await;
    let ctx = CoreContext::test_mock(fb);

    let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;

    // Derive both Hg and Git for master
    derive_data(&ctx, &repo, master_cs_id, true, true, true).await?;

    // Create bookmark "both_derived" - both Hg and Git
    let both_cs = CreateCommitContext::new(&ctx, &repo, vec![master_cs_id])
        .add_file("both", "content")
        .commit()
        .await?;
    derive_data(&ctx, &repo, both_cs, true, true, true).await?;
    bookmark(&ctx, &repo, "both_derived")
        .set_to(both_cs)
        .await?;
    bookmark(&ctx, &repo, "hg_only").set_to(both_cs).await?;

    // Create bookmark "hg_only" - only Hg derived at tip
    let hg_base = CreateCommitContext::new(&ctx, &repo, vec![both_cs])
        .add_file("hg_base", "content")
        .commit()
        .await?;
    derive_data(&ctx, &repo, hg_base, true, false, true).await?;
    bookmark(&ctx, &repo, "hg_only").set_to(hg_base).await?;

    let hg_tip = CreateCommitContext::new(&ctx, &repo, vec![hg_base])
        .add_file("hg_tip", "content")
        .commit()
        .await?;
    derive_data(&ctx, &repo, hg_tip, true, false, true).await?;
    bookmark(&ctx, &repo, "hg_only").set_to(hg_tip).await?;

    // Create bookmark "git_only" - only Git derived at tip
    let git_base = CreateCommitContext::new(&ctx, &repo, vec![both_cs])
        .add_file("git_base", "content")
        .commit()
        .await?;
    derive_data(&ctx, &repo, git_base, false, true, true).await?;
    bookmark(&ctx, &repo, "git_only").set_to(git_base).await?;

    let git_tip = CreateCommitContext::new(&ctx, &repo, vec![git_base])
        .add_file("git_tip", "content")
        .commit()
        .await?;
    derive_data(&ctx, &repo, git_tip, false, true, true).await?;
    bookmark(&ctx, &repo, "git_only").set_to(git_tip).await?;

    let warmers = setup_warmers(&ctx, &repo, WarmerConfig::all_warmers());
    let cache = init_cache_with_warmers(
        &ctx,
        &repo,
        Arc::try_unwrap(warmers).ok().unwrap(),
        InitMode::Rewind,
    )
    .await?;

    // Test HgOnly scope
    let hg_list = ScopedBookmarksCache::list(
        &cache,
        &ctx,
        &BookmarkPrefix::empty(),
        &BookmarkPagination::FromStart,
        None,
        WarmerRequirement::HgOnly,
    )
    .await?;

    let hg_bookmarks: HashMap<String, ChangesetId> = hg_list
        .into_iter()
        .map(|(name, (cs_id, _kind))| (name.to_string(), cs_id))
        .collect();

    assert!(
        hg_bookmarks.contains_key("master"),
        "HgOnly should include master"
    );
    assert!(
        hg_bookmarks.contains_key("both_derived"),
        "HgOnly should include both_derived"
    );
    assert!(
        hg_bookmarks.contains_key("hg_only"),
        "HgOnly should include hg_only"
    );
    assert_eq!(
        hg_bookmarks["hg_only"], hg_tip,
        "HgOnly should point hg_only to tip"
    );

    // Test GitOnly scope
    let git_list = ScopedBookmarksCache::list(
        &cache,
        &ctx,
        &BookmarkPrefix::empty(),
        &BookmarkPagination::FromStart,
        None,
        WarmerRequirement::GitOnly,
    )
    .await?;

    let git_bookmarks: HashMap<String, ChangesetId> = git_list
        .into_iter()
        .map(|(name, (cs_id, _kind))| (name.to_string(), cs_id))
        .collect();

    assert!(
        git_bookmarks.contains_key("master"),
        "GitOnly should include master"
    );
    assert!(
        git_bookmarks.contains_key("both_derived"),
        "GitOnly should include both_derived"
    );
    assert!(
        git_bookmarks.contains_key("git_only"),
        "GitOnly should include git_only"
    );
    assert_eq!(
        git_bookmarks["git_only"], git_tip,
        "GitOnly should point git_only to tip"
    );

    // For hg_only bookmark, GitOnly should rewind to base
    if let Some(&cs_id) = git_bookmarks.get("hg_only") {
        assert_eq!(cs_id, both_cs, "GitOnly should rewind hg_only to both_cs");
    }

    // Test AllKinds scope - most restrictive
    let all_list = ScopedBookmarksCache::list(
        &cache,
        &ctx,
        &BookmarkPrefix::empty(),
        &BookmarkPagination::FromStart,
        None,
        WarmerRequirement::AllKinds,
    )
    .await?;

    let all_bookmarks: HashMap<String, ChangesetId> = all_list
        .into_iter()
        .map(|(name, (cs_id, _kind))| (name.to_string(), cs_id))
        .collect();

    assert!(
        all_bookmarks.contains_key("master"),
        "AllKinds should include master"
    );
    assert!(
        all_bookmarks.contains_key("both_derived"),
        "AllKinds should include both_derived"
    );

    // Bookmarks with missing derivations should be rewound
    assert_eq!(
        all_bookmarks.get("hg_only"),
        Some(&both_cs),
        "AllKinds should rewind hg_only to base"
    );
    assert_eq!(
        all_bookmarks.get("git_only"),
        None,
        "AllKinds should not be derived anywhere in the bookmark"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_bookmark_deletion_with_scopes(fb: FacebookInit) -> Result<()> {
    // Test that bookmark deletion is handled correctly across scopes

    let repo: Repo = Linear::get_repo(fb).await;
    let ctx = CoreContext::test_mock(fb);

    let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
    derive_data(&ctx, &repo, master_cs_id, true, true, true).await?;

    // Create and derive a commit
    let commit = CreateCommitContext::new(&ctx, &repo, vec![master_cs_id])
        .add_file("file", "content")
        .commit()
        .await?;
    derive_data(&ctx, &repo, commit, true, true, true).await?;

    // Create bookmark
    bookmark(&ctx, &repo, "deletable").set_to(commit).await?;

    let warmers = setup_warmers(&ctx, &repo, WarmerConfig::all_warmers());
    let cache = init_cache_with_warmers(
        &ctx,
        &repo,
        Arc::try_unwrap(warmers).ok().unwrap(),
        InitMode::Rewind,
    )
    .await?;

    // Verify bookmark exists in all scopes
    let bookmark_key = BookmarkKey::new("deletable")?;
    assert_scoped_get(
        &cache,
        &ctx,
        &bookmark_key,
        Some(commit),
        Some(commit),
        Some(commit),
        "Before deletion",
    )
    .await?;

    // Delete bookmark
    bookmark(&ctx, &repo, "deletable").delete().await?;

    // Re-initialize cache
    let warmers2 = setup_warmers(&ctx, &repo, WarmerConfig::all_warmers());
    let cache_after_delete = init_cache_with_warmers(
        &ctx,
        &repo,
        Arc::try_unwrap(warmers2).ok().unwrap(),
        InitMode::Rewind,
    )
    .await?;

    // Verify bookmark is gone from all scopes
    assert_scoped_get(
        &cache_after_delete,
        &ctx,
        &bookmark_key,
        None,
        None,
        None,
        "After deletion",
    )
    .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_empty_history_bookmark(fb: FacebookInit) -> Result<()> {
    // Test bookmarks with no derivation history

    let repo: Repo = Linear::get_repo(fb).await;
    let ctx = CoreContext::test_mock(fb);

    let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;

    // Create a commit with no derivation
    let commit = CreateCommitContext::new(&ctx, &repo, vec![master_cs_id])
        .add_file("underived", "content")
        .commit()
        .await?;

    bookmark(&ctx, &repo, "underived_bookmark")
        .set_to(commit)
        .await?;

    let warmers = setup_warmers(&ctx, &repo, WarmerConfig::all_warmers());
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

    // Bookmark should not appear in any scope since nothing is derived
    let bookmark_key = BookmarkKey::new("underived_bookmark")?;
    assert_eq!(
        bookmarks.get(&bookmark_key),
        None,
        "Underived bookmark should not appear when nothing is derived"
    );

    // Now derive just Hg
    derive_data(&ctx, &repo, commit, true, false, true).await?;

    let warmers2 = setup_warmers(&ctx, &repo, WarmerConfig::all_warmers());
    let bookmarks_after_hg = init_bookmarks(
        &ctx,
        &*sub,
        repo.bookmarks(),
        repo.bookmark_update_log(),
        &warmers2,
        InitMode::Rewind,
    )
    .await?;

    let state_after_hg = bookmarks_after_hg
        .get(&bookmark_key)
        .expect("Bookmark should appear after Hg derivation");

    assert_bookmark_pointers(
        state_after_hg,
        Some(commit),
        None,
        None,
        "After Hg derivation only",
    );

    // Derive Git as well
    derive_data(&ctx, &repo, commit, false, true, false).await?;

    let warmers3 = setup_warmers(&ctx, &repo, WarmerConfig::all_warmers());
    let bookmarks_after_both = init_bookmarks(
        &ctx,
        &*sub,
        repo.bookmarks(),
        repo.bookmark_update_log(),
        &warmers3,
        InitMode::Rewind,
    )
    .await?;

    // Now it should appear in all scopes
    let state_after_both = bookmarks_after_both
        .get(&bookmark_key)
        .expect("Bookmark should appear after both derivations");

    assert_bookmark_pointers(
        state_after_both,
        Some(commit),
        Some(commit),
        Some(commit),
        "After both derivations",
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_transitive_derivation_effects(fb: FacebookInit) -> Result<()> {
    // Test that transitive derivation correctly affects scope pointers

    let repo: Repo = Linear::get_repo(fb).await;
    let ctx = CoreContext::test_mock(fb);

    let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;

    // Create a chain of commits
    let commit1 = CreateCommitContext::new(&ctx, &repo, vec![master_cs_id])
        .add_file("file1", "content1")
        .commit()
        .await?;

    let commit2 = CreateCommitContext::new(&ctx, &repo, vec![commit1])
        .add_file("file2", "content2")
        .commit()
        .await?;

    let commit3 = CreateCommitContext::new(&ctx, &repo, vec![commit2])
        .add_file("file3", "content3")
        .commit()
        .await?;

    // Only derive Hg and Unodes for commit1
    derive_data(&ctx, &repo, commit1, true, false, true).await?;

    // Create bookmark at commit1 to start building history
    bookmark(&ctx, &repo, "transitive_test")
        .create_publishing(commit1)
        .await?;

    // Derive Hg and Unodes for commit2 (but not Git yet - that will be transitive from commit3)
    derive_data(&ctx, &repo, commit2, true, false, false).await?;

    // Move bookmark to commit2
    bookmark(&ctx, &repo, "transitive_test")
        .set_to(commit2)
        .await?;

    // Move bookmark to commit3 (before deriving anything there)
    bookmark(&ctx, &repo, "transitive_test")
        .set_to(commit3)
        .await?;

    // Now derive Git for commit3 - this should transitively derive Git for commit1 and commit2
    derive_data(&ctx, &repo, commit3, false, true, true).await?;

    let warmers = setup_warmers(&ctx, &repo, WarmerConfig::all_warmers());
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

    let state = bookmarks
        .get(&BookmarkKey::new("transitive_test")?)
        .expect("Bookmark should exist");

    assert_bookmark_pointers(
        state,
        Some(commit2),
        Some(commit3),
        Some(commit2), // Git was derived transitively
        "Transitive derivation",
    );

    Ok(())
}
