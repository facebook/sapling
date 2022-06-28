/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod builder;
pub mod store;
mod subscription;
pub mod transaction;

pub use crate::builder::SqlBookmarksBuilder;
pub use crate::store::ArcSqlBookmarks;

#[cfg(test)]
mod test {
    use super::*;
    use anyhow::Result;
    use ascii::AsciiString;
    use bookmarks::Bookmark;
    use bookmarks::BookmarkKind;
    use bookmarks::BookmarkName;
    use bookmarks::BookmarkPagination;
    use bookmarks::BookmarkPrefix;
    use bookmarks::BookmarkUpdateReason;
    use bookmarks::Bookmarks;
    use bookmarks::Freshness;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use futures::stream::TryStreamExt;
    use mononoke_types::ChangesetId;
    use mononoke_types::RepositoryId;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;
    use mononoke_types_mocks::repo::REPO_ZERO;
    use quickcheck::quickcheck;
    use sql_construct::SqlConstruct;
    use std::collections::BTreeMap;
    use std::collections::HashSet;
    use tokio::runtime::Runtime;

    #[fbinit::test]
    async fn test_update_kind_compatibility(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let store = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let scratch_name = BookmarkName::new("book1").unwrap();
        let publishing_name = BookmarkName::new("book2").unwrap();
        let pull_default_name = BookmarkName::new("book3").unwrap();

        let conn = store.connections.write_connection.clone();

        let rows = vec![
            (
                &REPO_ZERO,
                &scratch_name,
                &ONES_CSID,
                &BookmarkKind::Scratch,
            ),
            (
                &REPO_ZERO,
                &publishing_name,
                &ONES_CSID,
                &BookmarkKind::Publishing,
            ),
            (
                &REPO_ZERO,
                &pull_default_name,
                &ONES_CSID,
                &BookmarkKind::PullDefaultPublishing,
            ),
        ];

        crate::transaction::insert_bookmarks(&conn, rows).await?;

        // Using 'create_scratch' to replace a non-scratch bookmark should fail.
        let mut txn = store.create_transaction(ctx.clone());
        txn.create_scratch(&publishing_name, ONES_CSID)?;
        assert!(!txn.commit().await?);

        // Using 'create' to replace a scratch bookmark should fail.
        let mut txn = store.create_transaction(ctx.clone());
        txn.create(&scratch_name, ONES_CSID, BookmarkUpdateReason::TestMove)?;
        assert!(!txn.commit().await?);

        // Using 'update_scratch' to update a publishing bookmark should fail.
        let mut txn = store.create_transaction(ctx.clone());
        txn.update_scratch(&publishing_name, TWOS_CSID, ONES_CSID)?;
        assert!(!txn.commit().await?);

        // Using 'update_scratch' to update a pull-default bookmark should fail.
        let mut txn = store.create_transaction(ctx.clone());
        txn.update_scratch(&pull_default_name, TWOS_CSID, ONES_CSID)?;
        assert!(!txn.commit().await?);

        // Using 'update' to update a publishing bookmark should succeed.
        let mut txn = store.create_transaction(ctx.clone());
        txn.update(
            &publishing_name,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;
        assert!(txn.commit().await?);

        // Using 'update' to update a pull-default bookmark should succeed.
        let mut txn = store.create_transaction(ctx.clone());
        txn.update(
            &pull_default_name,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;
        assert!(txn.commit().await?);

        // Using 'update' to update a scratch bookmark should fail.
        let mut txn = store.create_transaction(ctx.clone());
        txn.update(
            &scratch_name,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
        )?;
        assert!(!txn.commit().await?);

        // Using 'update_scratch' to update a scratch bookmark should succeed.
        let mut txn = store.create_transaction(ctx.clone());
        txn.update_scratch(&scratch_name, TWOS_CSID, ONES_CSID)?;
        assert!(txn.commit().await?);

        Ok(())
    }

    fn mock_bookmarks_response(
        bookmarks: &BTreeMap<BookmarkName, (BookmarkKind, ChangesetId)>,
        prefix: &BookmarkPrefix,
        kinds: &[BookmarkKind],
        pagination: &BookmarkPagination,
        limit: u64,
    ) -> Vec<(Bookmark, ChangesetId)> {
        let range = prefix.to_range().with_pagination(pagination.clone());
        bookmarks
            .range(range)
            .filter_map(|(bookmark, (kind, changeset_id))| {
                if kinds.iter().any(|k| kind == k) {
                    let bookmark = Bookmark {
                        name: bookmark.clone(),
                        kind: *kind,
                    };
                    Some((bookmark, *changeset_id))
                } else {
                    None
                }
            })
            .take(limit as usize)
            .collect()
    }

    fn insert_then_query(
        fb: FacebookInit,
        bookmarks: &BTreeMap<BookmarkName, (BookmarkKind, ChangesetId)>,
        query_freshness: Freshness,
        query_prefix: &BookmarkPrefix,
        query_kinds: &[BookmarkKind],
        query_pagination: &BookmarkPagination,
        query_limit: u64,
    ) -> Vec<(Bookmark, ChangesetId)> {
        let rt = Runtime::new().unwrap();

        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(123);

        let store = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(repo_id);
        let conn = store.connections.write_connection.clone();

        let rows = bookmarks
            .iter()
            .map(|(bookmark, (kind, changeset_id))| (&repo_id, bookmark, changeset_id, kind));

        rt.block_on(crate::transaction::insert_bookmarks(&conn, rows))
            .expect("insert failed");

        let response = store
            .list(
                ctx,
                query_freshness,
                query_prefix,
                query_kinds,
                query_pagination,
                query_limit,
            )
            .try_collect::<Vec<_>>();

        rt.block_on(response).expect("query failed")
    }

    quickcheck! {
        fn responses_match(
            fb: FacebookInit,
            bookmarks: BTreeMap<BookmarkName, (BookmarkKind, ChangesetId)>,
            freshness: Freshness,
            kinds: HashSet<BookmarkKind>,
            prefix_char: Option<ascii_ext::AsciiChar>,
            after: Option<BookmarkName>,
            limit: u64
        ) -> bool {
            // Test that requests return what is expected.
            let kinds: Vec<_> = kinds.into_iter().collect();
            let prefix = match prefix_char {
                Some(ch) => BookmarkPrefix::new_ascii(AsciiString::from(&[ch.0][..])),
                None => BookmarkPrefix::empty(),
            };
            let pagination = match after {
                Some(name) => BookmarkPagination::After(name),
                None => BookmarkPagination::FromStart,
            };
            let mut have = insert_then_query(
                fb,
                &bookmarks,
                freshness,
                &prefix,
                kinds.as_slice(),
                &pagination,
                limit,
            );
            let mut want = mock_bookmarks_response(
                &bookmarks,
                &prefix,
                kinds.as_slice(),
                &pagination,
                limit,
            );
            have.sort_by_key(|(_, csid)| *csid);
            want.sort_by_key(|(_, csid)| *csid);
            have == want
        }
    }
}
