// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Tests for the Filenodes store.

#![deny(warnings)]
#![feature(never_type)]

extern crate ascii;
extern crate async_unit;
extern crate bookmarks;
extern crate dbbookmarks;
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate mercurial_types;
extern crate mercurial_types_mocks;
extern crate tokio;

use bookmarks::{Bookmark, BookmarkPrefix};
use dbbookmarks::{MysqlDbBookmarks, SqliteDbBookmarks};
use mercurial_types_mocks::nodehash::{ONES_CSID, TWOS_CSID};
use mercurial_types_mocks::repo::{REPO_ONE, REPO_ZERO};

fn create_bookmark(book: &str) -> Bookmark {
    Bookmark::new(book.to_string()).unwrap()
}

fn create_prefix(book: &str) -> BookmarkPrefix {
    BookmarkPrefix::new(book.to_string()).unwrap()
}

macro_rules! bookmarks_test_impl {
    ($mod_name: ident => {
        new: $new_cb: expr,
    }) => {
        mod $mod_name {
            use super::*;

            use bookmarks::Bookmarks;
            use futures::future::Future;
            use futures::Stream;

            #[test]
            fn test_simple_unconditional_set_get() {
                let bookmarks = $new_cb();
                let name_correct = create_bookmark("book");
                let name_incorrect = create_bookmark("book2");

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.force_set(&name_correct, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().unwrap());

                assert_eq!(
                    bookmarks.get(&name_correct, &REPO_ZERO).wait().unwrap(),
                    Some(ONES_CSID)
                );
                assert_eq!(
                    bookmarks.get(&name_incorrect, &REPO_ZERO).wait().unwrap(),
                    None
                );
            }

            #[test]
            fn test_multi_unconditional_set_get() {
                let bookmarks = $new_cb();
                let name_1 = create_bookmark("book");
                let name_2 = create_bookmark("book2");

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.force_set(&name_1, &ONES_CSID).unwrap();
                txn.force_set(&name_2, &TWOS_CSID).unwrap();
                assert!(txn.commit().wait().unwrap());

                assert_eq!(
                    bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap(),
                    Some(ONES_CSID)
                );
                assert_eq!(
                    bookmarks.get(&name_2, &REPO_ZERO).wait().unwrap(),
                    Some(TWOS_CSID)
                );
            }

            #[test]
            fn test_unconditional_set_same_bookmark() {
                let bookmarks = $new_cb();
                let name_1 = create_bookmark("book");

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.force_set(&name_1, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().unwrap());

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.force_set(&name_1, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().unwrap());

                assert_eq!(
                    bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap(),
                    Some(ONES_CSID)
                );
            }

            #[test]
            fn test_simple_create() {
                let bookmarks = $new_cb();
                let name_1 = create_bookmark("book");

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.create(&name_1, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().unwrap());

                assert_eq!(
                    bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap(),
                    Some(ONES_CSID)
                );
            }

            #[test]
            fn test_create_already_existing() {
                let bookmarks = $new_cb();
                let name_1 = create_bookmark("book");

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.create(&name_1, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().unwrap());

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.create(&name_1, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().is_err());
            }

            #[test]
            fn test_create_change_same_bookmark() {
                let bookmarks = $new_cb();
                let name_1 = create_bookmark("book");

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.create(&name_1, &ONES_CSID).unwrap();
                assert!(txn.force_set(&name_1, &ONES_CSID).is_err());

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.force_set(&name_1, &ONES_CSID).unwrap();
                assert!(txn.create(&name_1, &ONES_CSID).is_err());

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.force_set(&name_1, &ONES_CSID).unwrap();
                assert!(txn.update(&name_1, &TWOS_CSID, &ONES_CSID).is_err());

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.update(&name_1, &TWOS_CSID, &ONES_CSID).unwrap();
                assert!(txn.force_set(&name_1, &ONES_CSID).is_err());

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.update(&name_1, &TWOS_CSID, &ONES_CSID).unwrap();
                assert!(txn.force_delete(&name_1).is_err());

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.force_delete(&name_1).unwrap();
                assert!(txn.update(&name_1, &TWOS_CSID, &ONES_CSID).is_err());

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.delete(&name_1, &ONES_CSID).unwrap();
                assert!(txn.update(&name_1, &TWOS_CSID, &ONES_CSID).is_err());

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.update(&name_1, &TWOS_CSID, &ONES_CSID).unwrap();
                assert!(txn.delete(&name_1, &ONES_CSID).is_err());
            }

            #[test]
            fn test_simple_update_bookmark() {
                let bookmarks = $new_cb();
                let name_1 = create_bookmark("book");

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.create(&name_1, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().unwrap());

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.update(&name_1, &TWOS_CSID, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().unwrap());

                assert_eq!(
                    bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap(),
                    Some(TWOS_CSID)
                );
            }

            #[test]
            fn test_update_non_existent_bookmark() {
                let bookmarks = $new_cb();
                let name_1 = create_bookmark("book");

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.update(&name_1, &TWOS_CSID, &ONES_CSID).unwrap();
                assert_eq!(txn.commit().wait().unwrap(), false);
            }

            #[test]
            fn test_update_existing_bookmark_with_incorrect_commit() {
                let bookmarks = $new_cb();
                let name_1 = create_bookmark("book");

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.create(&name_1, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().unwrap());

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.update(&name_1, &ONES_CSID, &TWOS_CSID).unwrap();
                assert_eq!(txn.commit().wait().unwrap(), false);
            }

            #[test]
            fn test_force_delete() {
                let bookmarks = $new_cb();
                let name_1 = create_bookmark("book");

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.force_delete(&name_1).unwrap();
                assert!(txn.commit().wait().unwrap());

                assert_eq!(bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap(), None);

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.create(&name_1, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().unwrap());
                assert!(bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap().is_some());

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.force_delete(&name_1).unwrap();
                assert!(txn.commit().wait().unwrap());

                assert_eq!(bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap(), None);
            }

            #[test]
            fn test_delete() {
                let bookmarks = $new_cb();
                let name_1 = create_bookmark("book");

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.delete(&name_1, &ONES_CSID).unwrap();
                assert_eq!(txn.commit().wait().unwrap(), false);

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.create(&name_1, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().unwrap());
                assert!(bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap().is_some());

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.delete(&name_1, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().unwrap());
            }

            #[test]
            fn test_delete_incorrect_hash() {
                let bookmarks = $new_cb();
                let name_1 = create_bookmark("book");

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.create(&name_1, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().unwrap());
                assert!(bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap().is_some());

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.delete(&name_1, &TWOS_CSID).unwrap();
                assert_eq!(txn.commit().wait().unwrap(), false);
            }

            #[test]
            fn test_list_by_prefix() {
                let bookmarks = $new_cb();
                let name_1 = create_bookmark("book1");
                let name_2 = create_bookmark("book2");

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.create(&name_1, &ONES_CSID).unwrap();
                txn.create(&name_2, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().unwrap());

                let prefix = create_prefix("book");
                let name_1_prefix = create_prefix("book1");
                let name_2_prefix = create_prefix("book2");
                assert_eq!(
                    bookmarks
                        .list_by_prefix(&prefix, &REPO_ZERO)
                        .collect()
                        .wait()
                        .unwrap(),
                    vec![(name_1.clone(), ONES_CSID), (name_2.clone(), ONES_CSID)]
                );

                assert_eq!(
                    bookmarks
                        .list_by_prefix(&name_1_prefix, &REPO_ZERO)
                        .collect()
                        .wait()
                        .unwrap(),
                    vec![(name_1.clone(), ONES_CSID)]
                );

                assert_eq!(
                    bookmarks
                        .list_by_prefix(&name_2_prefix, &REPO_ZERO)
                        .collect()
                        .wait()
                        .unwrap(),
                    vec![(name_2, ONES_CSID)]
                );
            }

            #[test]
            fn test_create_different_repos() {
                let bookmarks = $new_cb();
                let name_1 = create_bookmark("book");

                let mut txn = bookmarks.create_transaction(&REPO_ZERO);
                txn.force_set(&name_1, &ONES_CSID).unwrap();
                assert!(txn.commit().wait().is_ok());

                // Updating value from another repo, should fail
                let mut txn = bookmarks.create_transaction(&REPO_ONE);
                txn.update(&name_1, &TWOS_CSID, &ONES_CSID).unwrap();
                assert_eq!(txn.commit().wait().unwrap(), false);

                // Creating value should succeed
                let mut txn = bookmarks.create_transaction(&REPO_ONE);
                txn.create(&name_1, &TWOS_CSID).unwrap();
                assert!(txn.commit().wait().is_ok());

                assert_eq!(bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap(), Some(ONES_CSID));
                assert_eq!(bookmarks.get(&name_1, &REPO_ONE).wait().unwrap(), Some(TWOS_CSID));

                // Force deleting should delete only from one repo
                let mut txn = bookmarks.create_transaction(&REPO_ONE);
                txn.force_delete(&name_1).unwrap();
                assert!(txn.commit().wait().is_ok());
                assert_eq!(bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap(), Some(ONES_CSID));

                // delete should fail for another repo
                let mut txn = bookmarks.create_transaction(&REPO_ONE);
                txn.delete(&name_1, &ONES_CSID).unwrap();
                assert_eq!(txn.commit().wait().unwrap(), false);
            }
        }
    }
}

bookmarks_test_impl!(sqlite_tests => {
     new: create_sqlite,
 });

bookmarks_test_impl!(mysql_tests => {
     new: create_mysql,
 });

fn create_sqlite() -> SqliteDbBookmarks {
    SqliteDbBookmarks::in_memory().unwrap()
}

fn create_mysql() -> MysqlDbBookmarks {
    MysqlDbBookmarks::create_test_db("mononokefilenodestest").unwrap()
}
