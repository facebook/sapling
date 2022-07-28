/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests for the Bookmarks store.

use anyhow::Error;
use anyhow::Result;
use bookmarks::Bookmark;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateReason;
use bookmarks::Bookmarks;
use bookmarks::Freshness;
use context::CoreContext;
use dbbookmarks::SqlBookmarksBuilder;
use fbinit::FacebookInit;
use futures::stream::TryStreamExt;
use maplit::hashmap;
use mononoke_types::ChangesetId;
use mononoke_types::Timestamp;
use mononoke_types_mocks::changesetid::FIVES_CSID;
use mononoke_types_mocks::changesetid::FOURS_CSID;
use mononoke_types_mocks::changesetid::ONES_CSID;
use mononoke_types_mocks::changesetid::SIXES_CSID;
use mononoke_types_mocks::changesetid::THREES_CSID;
use mononoke_types_mocks::changesetid::TWOS_CSID;
use mononoke_types_mocks::repo::REPO_ONE;
use mononoke_types_mocks::repo::REPO_TWO;
use mononoke_types_mocks::repo::REPO_ZERO;
use quickcheck_arbitrary_derive::Arbitrary;
use sql::mysql_async::prelude::ConvIr;
use sql::mysql_async::Value;
use sql_construct::SqlConstruct;
use std::collections::HashMap;

fn create_bookmark_name(book: &str) -> BookmarkName {
    BookmarkName::new(book).unwrap()
}

fn create_prefix(book: &str) -> BookmarkPrefix {
    BookmarkPrefix::new(book).unwrap()
}

fn compare_log_entries(
    expected_entries: Vec<BookmarkUpdateLogEntry>,
    actual_entries: Vec<BookmarkUpdateLogEntry>,
) {
    assert_eq!(expected_entries.len(), actual_entries.len());
    for i in 0..expected_entries.len() {
        let expected = expected_entries.get(i).unwrap();
        let actual = actual_entries.get(i).unwrap();
        assert_eq!(expected.id, actual.id);
        assert_eq!(expected.repo_id, actual.repo_id);
        assert_eq!(expected.bookmark_name, actual.bookmark_name);
        assert_eq!(expected.to_changeset_id, actual.to_changeset_id);
        assert_eq!(expected.from_changeset_id, actual.from_changeset_id);
        assert_eq!(expected.reason, actual.reason);
    }
}

#[fbinit::test]
async fn test_simple_unconditional_set_get(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_correct = create_bookmark_name("book");
    let name_incorrect = create_bookmark_name("book2");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_set(&name_correct, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());

    assert_eq!(
        bookmarks.get_raw(ctx.clone(), &name_correct).await.unwrap(),
        Some((ONES_CSID, Some(1)))
    );
    assert_eq!(
        bookmarks
            .get_raw(ctx.clone(), &name_incorrect)
            .await
            .unwrap(),
        None
    );

    compare_log_entries(
        bookmarks
            .read_next_bookmark_log_entries(ctx.clone(), 0, 1, Freshness::MostRecent)
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
        vec![BookmarkUpdateLogEntry {
            id: 1,
            repo_id: REPO_ZERO,
            bookmark_name: name_correct,
            to_changeset_id: Some(ONES_CSID),
            from_changeset_id: None,
            reason: BookmarkUpdateReason::TestMove,
            timestamp: Timestamp::now(),
        }],
    );
}

#[fbinit::test]
async fn test_multi_unconditional_set_get(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");
    let name_2 = create_bookmark_name("book2");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    txn.force_set(&name_2, TWOS_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());

    assert_eq!(
        bookmarks.get_raw(ctx.clone(), &name_1).await.unwrap(),
        Some((ONES_CSID, Some(1)))
    );

    assert_eq!(
        bookmarks.get_raw(ctx.clone(), &name_2).await.unwrap(),
        Some((TWOS_CSID, Some(2)))
    );
}

#[fbinit::test]
async fn test_unconditional_set_same_bookmark(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());

    assert_eq!(
        bookmarks.get_raw(ctx.clone(), &name_1).await.unwrap(),
        Some((ONES_CSID, Some(2)))
    );
}

#[fbinit::test]
async fn test_simple_create(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());

    assert_eq!(
        bookmarks.get_raw(ctx.clone(), &name_1).await.unwrap(),
        Some((ONES_CSID, Some(1)))
    );

    compare_log_entries(
        bookmarks
            .read_next_bookmark_log_entries(ctx.clone(), 0, 1, Freshness::MostRecent)
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
        vec![BookmarkUpdateLogEntry {
            id: 1,
            repo_id: REPO_ZERO,
            bookmark_name: name_1,
            to_changeset_id: Some(ONES_CSID),
            from_changeset_id: None,
            reason: BookmarkUpdateReason::TestMove,
            timestamp: Timestamp::now(),
        }],
    );
}

#[fbinit::test]
async fn test_create_already_existing(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(!txn.commit().await.unwrap());
}

#[fbinit::test]
async fn test_create_change_same_bookmark(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(
        txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
            .is_err()
    );

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(
        txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
            .is_err()
    );

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(
        txn.update(
            &name_1,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )
        .is_err()
    );

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        TWOS_CSID,
        ONES_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    assert!(
        txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
            .is_err()
    );

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        TWOS_CSID,
        ONES_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    assert!(
        txn.force_delete(&name_1, BookmarkUpdateReason::TestMove)
            .is_err()
    );

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_delete(&name_1, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(
        txn.update(
            &name_1,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )
        .is_err()
    );

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.delete(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(
        txn.update(
            &name_1,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )
        .is_err()
    );

    let mut txn = bookmarks.create_transaction(ctx);
    txn.update(
        &name_1,
        TWOS_CSID,
        ONES_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    assert!(
        txn.delete(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
            .is_err()
    );
}

#[fbinit::test]
async fn test_simple_update_bookmark(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        TWOS_CSID,
        ONES_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    assert!(txn.commit().await.unwrap());

    assert_eq!(
        bookmarks.get_raw(ctx.clone(), &name_1).await.unwrap(),
        Some((TWOS_CSID, Some(2)))
    );

    compare_log_entries(
        bookmarks
            .read_next_bookmark_log_entries(ctx.clone(), 1, 1, Freshness::MostRecent)
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
        vec![BookmarkUpdateLogEntry {
            id: 2,
            repo_id: REPO_ZERO,
            bookmark_name: name_1,
            to_changeset_id: Some(TWOS_CSID),
            from_changeset_id: Some(ONES_CSID),
            reason: BookmarkUpdateReason::TestMove,
            timestamp: Timestamp::now(),
        }],
    );
}

#[fbinit::test]
async fn test_noop_update(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        ONES_CSID,
        ONES_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    assert!(txn.commit().await.unwrap());

    assert_eq!(
        bookmarks.get_raw(ctx.clone(), &name_1).await.unwrap(),
        Some((ONES_CSID, Some(2)))
    );
}

#[fbinit::test]
async fn test_scratch_update_bookmark(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create_scratch(&name_1, ONES_CSID).unwrap();
    assert!(txn.commit().await.unwrap());

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update_scratch(&name_1, TWOS_CSID, ONES_CSID).unwrap();
    assert!(txn.commit().await.unwrap());

    assert_eq!(
        bookmarks.get_raw(ctx.clone(), &name_1).await.unwrap(),
        Some((TWOS_CSID, None))
    );

    compare_log_entries(
        bookmarks
            .read_next_bookmark_log_entries(ctx.clone(), 1, 1, Freshness::MostRecent)
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
        vec![],
    );
}

#[fbinit::test]
async fn test_update_non_existent_bookmark(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        TWOS_CSID,
        ONES_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    assert!(!txn.commit().await.unwrap());
}

#[fbinit::test]
async fn test_update_existing_bookmark_with_incorrect_commit(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        ONES_CSID,
        TWOS_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    assert!(!txn.commit().await.unwrap());
}

#[fbinit::test]
async fn test_force_delete(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_delete(&name_1, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());

    assert_eq!(bookmarks.get_raw(ctx.clone(), &name_1).await.unwrap(), None);

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());
    assert_eq!(
        bookmarks.get_raw(ctx.clone(), &name_1).await.unwrap(),
        Some((ONES_CSID, Some(2)))
    );

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_delete(&name_1, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());

    assert_eq!(bookmarks.get_raw(ctx.clone(), &name_1).await.unwrap(), None);

    compare_log_entries(
        bookmarks
            .read_next_bookmark_log_entries(ctx.clone(), 2, 1, Freshness::MostRecent)
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
        vec![BookmarkUpdateLogEntry {
            id: 3,
            repo_id: REPO_ZERO,
            bookmark_name: name_1,
            to_changeset_id: None,
            from_changeset_id: None,
            reason: BookmarkUpdateReason::TestMove,
            timestamp: Timestamp::now(),
        }],
    );
}

#[fbinit::test]
async fn test_delete(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.delete(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(!txn.commit().await.unwrap());

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());
    assert_eq!(
        bookmarks.get_raw(ctx.clone(), &name_1).await.unwrap(),
        Some((ONES_CSID, Some(1)))
    );

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.delete(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());

    compare_log_entries(
        bookmarks
            .read_next_bookmark_log_entries(ctx.clone(), 1, 1, Freshness::MostRecent)
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
        vec![BookmarkUpdateLogEntry {
            id: 2,
            repo_id: REPO_ZERO,
            bookmark_name: name_1,
            to_changeset_id: None,
            from_changeset_id: Some(ONES_CSID),
            reason: BookmarkUpdateReason::TestMove,
            timestamp: Timestamp::now(),
        }],
    );
}

#[fbinit::test]
async fn test_delete_incorrect_hash(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());
    assert_eq!(
        bookmarks.get_raw(ctx.clone(), &name_1).await.unwrap(),
        Some((ONES_CSID, Some(1)))
    );

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.delete(&name_1, TWOS_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(!txn.commit().await.unwrap());
}

#[fbinit::test]
async fn test_list_by_prefix(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book1");
    let name_2 = create_bookmark_name("book2");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    txn.create(&name_2, TWOS_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());

    let prefix = create_prefix("book");
    let name_1_prefix = create_prefix("book1");
    let name_2_prefix = create_prefix("book2");

    assert_eq!(
        bookmarks
            .list(
                ctx.clone(),
                Freshness::MostRecent,
                &prefix,
                BookmarkKind::ALL,
                &BookmarkPagination::FromStart,
                std::u64::MAX
            )
            .try_collect::<HashMap<_, _>>()
            .await
            .unwrap(),
        hashmap! {
            Bookmark::new(name_1.clone(), BookmarkKind::PullDefaultPublishing) => ONES_CSID,
            Bookmark::new(name_2.clone(), BookmarkKind::PullDefaultPublishing) => TWOS_CSID
        }
    );

    assert_eq!(
        bookmarks
            .list(
                ctx.clone(),
                Freshness::MostRecent,
                &name_1_prefix,
                BookmarkKind::ALL,
                &BookmarkPagination::FromStart,
                std::u64::MAX
            )
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
        vec![(
            Bookmark::new(name_1.clone(), BookmarkKind::PullDefaultPublishing),
            ONES_CSID
        )]
    );

    assert_eq!(
        bookmarks
            .list(
                ctx.clone(),
                Freshness::MostRecent,
                &name_2_prefix,
                BookmarkKind::ALL,
                &BookmarkPagination::FromStart,
                std::u64::MAX
            )
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
        vec![(
            Bookmark::new(name_2.clone(), BookmarkKind::PullDefaultPublishing),
            TWOS_CSID
        )]
    );
}

#[fbinit::test]
async fn test_create_different_repos(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let builder = SqlBookmarksBuilder::with_sqlite_in_memory().unwrap();
    let bookmarks_0 = builder.clone().with_repo_id(REPO_ZERO);
    let bookmarks_1 = builder.with_repo_id(REPO_ONE);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks_0.create_transaction(ctx.clone());
    txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.is_ok());

    // Updating value from another repo, should fail
    let mut txn = bookmarks_1.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        TWOS_CSID,
        ONES_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    assert!(!txn.commit().await.unwrap());

    // Creating value should succeed
    let mut txn = bookmarks_1.create_transaction(ctx.clone());
    txn.create(&name_1, TWOS_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.is_ok());

    assert_eq!(
        bookmarks_0.get(ctx.clone(), &name_1).await.unwrap(),
        Some(ONES_CSID)
    );
    assert_eq!(
        bookmarks_1.get(ctx.clone(), &name_1).await.unwrap(),
        Some(TWOS_CSID)
    );

    // Force deleting should delete only from one repo
    let mut txn = bookmarks_1.create_transaction(ctx.clone());
    txn.force_delete(&name_1, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.is_ok());
    assert_eq!(
        bookmarks_0.get(ctx.clone(), &name_1).await.unwrap(),
        Some(ONES_CSID)
    );

    // delete should fail for another repo
    let mut txn = bookmarks_1.create_transaction(ctx.clone());
    txn.delete(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(!txn.commit().await.unwrap());
}

async fn fetch_single(
    fb: FacebookInit,
    bookmarks: &dyn BookmarkUpdateLog,
    id: u64,
) -> BookmarkUpdateLogEntry {
    let ctx = CoreContext::test_mock(fb);
    bookmarks
        .read_next_bookmark_log_entries(ctx, id, 1, Freshness::MostRecent)
        .try_collect::<Vec<_>>()
        .await
        .unwrap()
        .get(0)
        .unwrap()
        .clone()
}

#[fbinit::test]
async fn test_log_correct_order(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");
    let name_2 = create_bookmark_name("book2");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.is_ok());

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        TWOS_CSID,
        ONES_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    txn.commit().await.unwrap();

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        THREES_CSID,
        TWOS_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    txn.commit().await.unwrap();

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        FOURS_CSID,
        THREES_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    txn.commit().await.unwrap();

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_set(&name_2, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.is_ok());

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        FIVES_CSID,
        FOURS_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    txn.commit().await.unwrap();

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        SIXES_CSID,
        FIVES_CSID,
        BookmarkUpdateReason::Pushrebase,
    )
    .unwrap();
    txn.commit().await.unwrap();

    let log_entry = fetch_single(fb, &bookmarks, 0).await;
    assert_eq!(log_entry.to_changeset_id.unwrap(), ONES_CSID);

    let log_entry = fetch_single(fb, &bookmarks, 1).await;
    assert_eq!(log_entry.to_changeset_id.unwrap(), TWOS_CSID);

    let log_entry = fetch_single(fb, &bookmarks, 2).await;
    assert_eq!(log_entry.to_changeset_id.unwrap(), THREES_CSID);

    let log_entry = fetch_single(fb, &bookmarks, 3).await;
    assert_eq!(log_entry.to_changeset_id.unwrap(), FOURS_CSID);

    let log_entry = fetch_single(fb, &bookmarks, 5).await;
    assert_eq!(log_entry.to_changeset_id.unwrap(), FIVES_CSID);

    assert_eq!(
        bookmarks
            .read_next_bookmark_log_entries(ctx.clone(), 0, 4, Freshness::MostRecent)
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .len(),
        4
    );

    assert_eq!(
        bookmarks
            .read_next_bookmark_log_entries(ctx.clone(), 0, 8, Freshness::MostRecent)
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .len(),
        7
    );

    let entries = bookmarks
        .read_next_bookmark_log_entries(ctx.clone(), 0, 6, Freshness::MostRecent)
        .try_collect::<Vec<_>>()
        .await
        .unwrap();

    let cs_ids: Vec<_> = entries
        .into_iter()
        .map(|entry| entry.to_changeset_id.unwrap())
        .collect();
    assert_eq!(
        cs_ids,
        vec![
            ONES_CSID,
            TWOS_CSID,
            THREES_CSID,
            FOURS_CSID,
            ONES_CSID,
            FIVES_CSID
        ]
    );

    let entries = bookmarks
        .read_next_bookmark_log_entries_same_bookmark_and_reason(ctx.clone(), 0, 6)
        .try_collect::<Vec<_>>()
        .await
        .unwrap();

    // FOURS_CSID -> FIVES_CSID update is missing, because it has a different bookmark
    let cs_ids: Vec<_> = entries
        .into_iter()
        .map(|entry| entry.to_changeset_id.unwrap())
        .collect();
    assert_eq!(cs_ids, vec![ONES_CSID, TWOS_CSID, THREES_CSID, FOURS_CSID]);

    let entries = bookmarks
        .read_next_bookmark_log_entries_same_bookmark_and_reason(ctx.clone(), 5, 6)
        .try_collect::<Vec<_>>()
        .await
        .unwrap();

    // FIVES_CSID -> SIXES_CSID update is missing, because it has a different reason
    let cs_ids: Vec<_> = entries
        .into_iter()
        .map(|entry| entry.to_changeset_id.unwrap())
        .collect();
    assert_eq!(cs_ids, vec![FIVES_CSID]);
}

#[fbinit::test]
async fn test_read_log_entry_many_repos(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let builder = SqlBookmarksBuilder::with_sqlite_in_memory().unwrap();
    let bookmarks_0 = builder.clone().with_repo_id(REPO_ZERO);
    let bookmarks_1 = builder.clone().with_repo_id(REPO_ONE);
    let bookmarks_2 = builder.with_repo_id(REPO_TWO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks_0.create_transaction(ctx.clone());
    txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.is_ok());

    let mut txn = bookmarks_1.create_transaction(ctx.clone());
    txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.is_ok());

    assert_eq!(
        bookmarks_0
            .read_next_bookmark_log_entries(ctx.clone(), 0, 1, Freshness::MostRecent)
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .len(),
        1
    );

    assert_eq!(
        bookmarks_1
            .read_next_bookmark_log_entries(ctx.clone(), 0, 1, Freshness::MostRecent)
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .len(),
        1
    );

    assert_eq!(
        bookmarks_0
            .read_next_bookmark_log_entries(ctx.clone(), 1, 1, Freshness::MostRecent)
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .len(),
        0
    );

    assert_eq!(
        bookmarks_2
            .read_next_bookmark_log_entries(ctx.clone(), 0, 1, Freshness::MostRecent)
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn test_update_reason_conversion() -> Result<(), Error> {
    use BookmarkUpdateReason::*;
    let unusedreason = TestMove;

    // If you are adding a new variant here, please also add a test
    // for the new bookmark reason.
    match unusedreason {
        Backsyncer => {}
        Blobimport => {}
        ManualMove => {}
        Push => {}
        Pushrebase => {}
        TestMove => {}
        XRepoSync => {}
        ApiRequest => {}
    };

    let reasons = vec![
        Backsyncer, Blobimport, ManualMove, Push, Pushrebase, TestMove, XRepoSync, ApiRequest,
    ];

    for reason in reasons {
        let value = Value::from(reason);
        BookmarkUpdateReason::new(value)?;
    }

    Ok(())
}

#[fbinit::test]
async fn test_list_bookmark_log_entries(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.is_ok());

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        TWOS_CSID,
        ONES_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    txn.commit().await.unwrap();

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        THREES_CSID,
        TWOS_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    txn.commit().await.unwrap();

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        FOURS_CSID,
        THREES_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    txn.commit().await.unwrap();

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        FIVES_CSID,
        FOURS_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    txn.commit().await.unwrap();

    assert_eq!(
        bookmarks
            .list_bookmark_log_entries(ctx.clone(), name_1.clone(), 3, None, Freshness::MostRecent)
            .map_ok(|(_id, cs, rs, _ts)| (cs, rs))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
        vec![
            (Some(FIVES_CSID), BookmarkUpdateReason::TestMove),
            (Some(FOURS_CSID), BookmarkUpdateReason::TestMove),
            (Some(THREES_CSID), BookmarkUpdateReason::TestMove),
        ]
    );

    let current_timestamp = Timestamp::now();
    let day_old_timestamp =
        Timestamp::from_timestamp_secs(current_timestamp.timestamp_seconds() - 86400);
    assert_eq!(
        bookmarks
            .list_bookmark_log_entries_ts_in_range(
                ctx.clone(),
                name_1.clone(),
                3,
                day_old_timestamp,
                current_timestamp,
            )
            .map_ok(|(_id, cs, rs, _ts)| (cs, rs))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
        vec![
            (Some(FIVES_CSID), BookmarkUpdateReason::TestMove),
            (Some(FOURS_CSID), BookmarkUpdateReason::TestMove),
            (Some(THREES_CSID), BookmarkUpdateReason::TestMove),
        ]
    );

    assert_eq!(
        bookmarks
            .list_bookmark_log_entries(ctx.clone(), name_1, 3, Some(1), Freshness::MostRecent)
            .map_ok(|(_id, cs, rs, _ts)| (cs, rs))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
        vec![
            (Some(FOURS_CSID), BookmarkUpdateReason::TestMove),
            (Some(THREES_CSID), BookmarkUpdateReason::TestMove),
            (Some(TWOS_CSID), BookmarkUpdateReason::TestMove),
        ]
    );
}

#[fbinit::test]
async fn test_get_largest_log_id(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    assert_eq!(
        bookmarks
            .get_largest_log_id(ctx.clone(), Freshness::MostRecent)
            .await
            .unwrap(),
        None
    );
    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();

    assert!(txn.commit().await.is_ok());
    assert_eq!(
        bookmarks
            .get_largest_log_id(ctx.clone(), Freshness::MostRecent)
            .await
            .unwrap(),
        Some(1)
    );

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        TWOS_CSID,
        ONES_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    txn.commit().await.unwrap();

    assert_eq!(
        bookmarks
            .get_largest_log_id(ctx.clone(), Freshness::MostRecent)
            .await
            .unwrap(),
        Some(2)
    );

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        THREES_CSID,
        TWOS_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    txn.commit().await.unwrap();

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        FOURS_CSID,
        THREES_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    txn.commit().await.unwrap();

    assert_eq!(
        bookmarks
            .get_largest_log_id(ctx.clone(), Freshness::MostRecent)
            .await
            .unwrap(),
        Some(4)
    );
}

#[fbinit::test]
async fn test_creating_publishing_bookmarks(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create_publishing(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());
    assert_eq!(
        bookmarks
            .list(
                ctx.clone(),
                Freshness::MostRecent,
                &BookmarkPrefix::empty(),
                BookmarkKind::ALL,
                &BookmarkPagination::FromStart,
                std::u64::MAX
            )
            .try_collect::<HashMap<_, _>>()
            .await
            .unwrap(),
        hashmap! {
            Bookmark::new(name_1.clone(), BookmarkKind::Publishing) => ONES_CSID,
        }
    );

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(
        &name_1,
        TWOS_CSID,
        ONES_CSID,
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    assert!(txn.commit().await.unwrap());

    assert_eq!(
        bookmarks
            .list(
                ctx.clone(),
                Freshness::MostRecent,
                &BookmarkPrefix::empty(),
                BookmarkKind::ALL,
                &BookmarkPagination::FromStart,
                std::u64::MAX
            )
            .try_collect::<HashMap<_, _>>()
            .await
            .unwrap(),
        hashmap! {
            Bookmark::new(name_1.clone(), BookmarkKind::Publishing) => TWOS_CSID,
        }
    );
}

#[fbinit::test]
async fn test_pagination_ordering(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let name_1 = create_bookmark_name("book1");
    let name_2 = create_bookmark_name("book2");
    let name_3 = create_bookmark_name("book3");

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create_publishing(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    txn.create_publishing(&name_2, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    txn.create_publishing(&name_3, ONES_CSID, BookmarkUpdateReason::TestMove)
        .unwrap();
    assert!(txn.commit().await.unwrap());

    // If the code breaks and these results become unordered then that will happen non
    // deterministically. Call list() many times to ensure that the bookmarks are actually
    // ordered.
    for _ in 0..10 {
        assert_eq!(
            bookmarks
                .list(
                    ctx.clone(),
                    Freshness::MostRecent,
                    &BookmarkPrefix::empty(),
                    BookmarkKind::ALL,
                    &BookmarkPagination::FromStart,
                    3
                )
                .try_collect::<Vec<(_, _)>>()
                .await
                .unwrap(),
            vec![
                (
                    Bookmark::new(name_1.clone(), BookmarkKind::Publishing),
                    ONES_CSID
                ),
                (
                    Bookmark::new(name_2.clone(), BookmarkKind::Publishing),
                    ONES_CSID
                ),
                (
                    Bookmark::new(name_3.clone(), BookmarkKind::Publishing),
                    ONES_CSID
                ),
            ]
        );

        assert_eq!(
            bookmarks
                .list(
                    ctx.clone(),
                    Freshness::MostRecent,
                    &BookmarkPrefix::empty(),
                    BookmarkKind::ALL,
                    &BookmarkPagination::After(name_1.clone()),
                    1
                )
                .try_collect::<Vec<(_, _)>>()
                .await
                .unwrap()[0],
            (
                Bookmark::new(name_2.clone(), BookmarkKind::Publishing),
                ONES_CSID
            )
        );
    }
}

#[fbinit::test]
async fn bookmark_subscription_initialization(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()?.with_repo_id(REPO_ZERO);
    let book1 = create_bookmark_name("book1");
    let book2 = create_bookmark_name("book2");
    let book3 = create_bookmark_name("book3");

    // Create some history we won't care about.

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create_publishing(&book1, ONES_CSID, BookmarkUpdateReason::TestMove)?;
    assert!(txn.commit().await?);

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_delete(&book1, BookmarkUpdateReason::TestMove)?;
    assert!(txn.commit().await?);

    // Create some bookmarks now that we're going to keep.

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create_publishing(&book1, ONES_CSID, BookmarkUpdateReason::TestMove)?;
    txn.create_publishing(&book2, TWOS_CSID, BookmarkUpdateReason::TestMove)?;
    assert!(txn.commit().await?);

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create_publishing(&book3, THREES_CSID, BookmarkUpdateReason::TestMove)?;
    assert!(txn.commit().await?);

    let mut sub = bookmarks
        .create_subscription(&ctx, Freshness::MostRecent)
        .await?;

    sub.refresh(&ctx).await?;
    assert_eq!(
        *sub.bookmarks(),
        hashmap! {
            book1.clone() => (ONES_CSID, BookmarkKind::Publishing),
            book2.clone() => (TWOS_CSID, BookmarkKind::Publishing),
            book3.clone() => (THREES_CSID, BookmarkKind::Publishing),
        }
    );

    Ok(())
}

#[fbinit::test]
async fn bookmark_subscription_updates(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()?.with_repo_id(REPO_ZERO);
    let book = create_bookmark_name("book");

    let mut sub = bookmarks
        .create_subscription(&ctx, Freshness::MostRecent)
        .await?;

    assert_eq!(*sub.bookmarks(), hashmap! {});

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.create_publishing(&book, ONES_CSID, BookmarkUpdateReason::TestMove)?;
    assert!(txn.commit().await?);

    sub.refresh(&ctx).await?;
    assert_eq!(
        *sub.bookmarks(),
        hashmap! { book.clone() => (ONES_CSID, BookmarkKind::Publishing)}
    );

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.update(&book, TWOS_CSID, ONES_CSID, BookmarkUpdateReason::TestMove)?;
    assert!(txn.commit().await?);

    sub.refresh(&ctx).await?;
    assert_eq!(
        *sub.bookmarks(),
        hashmap! { book.clone() => (TWOS_CSID, BookmarkKind::Publishing)}
    );

    let mut txn = bookmarks.create_transaction(ctx.clone());
    txn.force_delete(&book, BookmarkUpdateReason::TestMove)?;
    assert!(txn.commit().await?);

    sub.refresh(&ctx).await?;
    assert_eq!(*sub.bookmarks(), hashmap! {});

    Ok(())
}

#[derive(Arbitrary, Clone, Copy, Debug)]
enum TestBookmark {
    Book1,
    Book2,
}

#[derive(Arbitrary, Clone, Copy, Debug)]
enum BookmarkOp {
    /// Set this bookmark.
    Set(ChangesetId),
    /// ForceSet this bookmark (this also changes the kind)
    ForceSet(ChangesetId),
    /// Delete this bookmark
    Delete,
}

/// Use Quickcheck to produce a test scenario of bookmark updates.
#[derive(Arbitrary, Clone, Copy, Debug)]
enum TestOp {
    /// Update one of our test bookmarks
    Bookmark(TestBookmark, BookmarkOp),
    /// Do nothing. This allows multiple refreshes to occur in sequence.
    Noop,
    /// Update the BookmarksSubscription and check that it returns the right bookmark values.
    Refresh,
}

/// Verify bookmark subscriptions using Quickcheck. We create a test scenario and verify that the
/// bookmark subscriptions returns the same data it would return if it was freshly created now (we
/// test that this satisfies our assumptions in separate tests for the bookmarks subscription).
#[fbinit::test]
fn bookmark_subscription_quickcheck(fb: FacebookInit) {
    #[tokio::main(flavor = "current_thread")]
    async fn check(fb: FacebookInit, mut ops: Vec<TestOp>) -> bool {
        async move {
            ops.push(TestOp::Refresh);

            let ctx = CoreContext::test_mock(fb);

            let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()?.with_repo_id(REPO_ZERO);

            let book1 = create_bookmark_name("book1");
            let book2 = create_bookmark_name("book2");

            let mut book1_id = None;
            let mut book2_id = None;

            let mut sub = bookmarks
                .create_subscription(&ctx, Freshness::MostRecent)
                .await?;

            for op in ops {
                match op {
                    TestOp::Bookmark(book, op) => {
                        let current_cs_id = match book {
                            TestBookmark::Book1 => &mut book1_id,
                            TestBookmark::Book2 => &mut book2_id,
                        };

                        let book = match book {
                            TestBookmark::Book1 => &book1,
                            TestBookmark::Book2 => &book2,
                        };

                        let mut txn = bookmarks.create_transaction(ctx.clone());

                        match op {
                            BookmarkOp::Set(cs_id) => {
                                match *current_cs_id {
                                    Some(current_cs_id) => {
                                        txn.update(
                                            book,
                                            cs_id,
                                            current_cs_id,
                                            BookmarkUpdateReason::TestMove,
                                        )?;
                                    }
                                    None => {
                                        txn.create_publishing(
                                            book,
                                            cs_id,
                                            BookmarkUpdateReason::TestMove,
                                        )?;
                                    }
                                }

                                *current_cs_id = Some(cs_id);
                            }
                            BookmarkOp::ForceSet(cs_id) => {
                                txn.force_set(book, cs_id, BookmarkUpdateReason::TestMove)?;
                                *current_cs_id = Some(cs_id);
                            }
                            BookmarkOp::Delete => {
                                match *current_cs_id {
                                    Some(current_cs_id) => {
                                        txn.delete(
                                            book,
                                            current_cs_id,
                                            BookmarkUpdateReason::TestMove,
                                        )?;
                                    }
                                    None => {
                                        txn.force_delete(book, BookmarkUpdateReason::TestMove)?;
                                    }
                                }

                                *current_cs_id = None;
                            }
                        };

                        assert!(txn.commit().await?);
                    }
                    TestOp::Noop => {
                        // It's a noop
                    }
                    TestOp::Refresh => {
                        sub.refresh(&ctx).await?;

                        let control = bookmarks
                            .create_subscription(&ctx, Freshness::MostRecent)
                            .await?;

                        if control.bookmarks() != sub.bookmarks() {
                            return Ok(false);
                        }
                    }
                };
            }

            Result::<_, Error>::Ok(true)
        }
        .await
        .unwrap()
    }

    quickcheck::quickcheck(check as fn(FacebookInit, Vec<TestOp>) -> bool);

    // We need to hold the FacebookInit until this point because Arbitrary for FacebookInit just
    // assumes init!
    drop(fb)
}
