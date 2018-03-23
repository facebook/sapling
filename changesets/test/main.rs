// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Tests for the Changesets store.

#![deny(warnings)]

#[macro_use]
extern crate assert_matches;
extern crate diesel;
extern crate failure_ext as failure;
extern crate futures;
extern crate tokio;

extern crate changesets;
extern crate mercurial_types_mocks;

use std::sync::Arc;

use failure::Error;
use futures::Future;

use changesets::{ChangesetEntry, ChangesetInsert, Changesets, ErrorKind, MysqlChangesets,
                 SqliteChangesets};
use mercurial_types_mocks::nodehash::*;
use mercurial_types_mocks::repo::*;

fn add_and_get<C: Changesets + 'static>(changesets: C) {
    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };
    let test = changesets
        .add(row)
        .map_err(|err| panic!("Adding new entry failed: {:?}", err))
        .and_then(move |_| changesets.get(REPO_ZERO, ONES_CSID))
        .map_err(|err| panic!("Get failed: {:?}", err))
        .map(|result| {
            assert_eq!(
                result,
                Some(ChangesetEntry {
                    repo_id: REPO_ZERO,
                    cs_id: ONES_CSID,
                    parents: vec![],
                    gen: 1,
                })
            )
        });
    tokio::run(test);
}

fn add_missing_parents<C: Changesets>(changesets: C) {
    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![TWOS_CSID],
    };
    let test = changesets
        .add(row)
        .map(|err| {
            panic!(
                "Adding entry with missing parents succeeded (should have failed): {:?}",
                err
            )
        })
        .map_err(|result| {
            assert_matches!(
                result.downcast::<ErrorKind>(),
                Ok(ErrorKind::MissingParents(ref x)) if x == &vec![TWOS_CSID])
        });
    tokio::run(test);
}

fn missing<C: Changesets + 'static>(changesets: C) {
    let test = changesets
        .get(REPO_ZERO, ONES_CSID)
        .map_err(|err| {
            panic!(
                "Failed to fetch missing changeset (should succeed with None instead): {:?}",
                err
            )
        })
        .map(|result| assert_eq!(result, None));
    tokio::run(test);
}

fn duplicate<C: Changesets + 'static>(changesets: C) {
    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };
    let test = changesets
        .add(row.clone())
        .map_err(|err| panic!("Adding new entry failed: {:?}", err))
        .and_then({ move |_| changesets.add(row) })
        .map(|_| panic!("Adding duplicate entry succeeded (should fail)"))
        .map_err(|result: Error| match result.downcast::<ErrorKind>() {
            Ok(ErrorKind::DuplicateChangeset) => {}
            err => panic!("unexpected error: {:?}", err),
        });
    tokio::run(test);
}

fn complex<C: Changesets + Clone + 'static>(changesets: C) {
    let row1 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };
    let test = changesets
        .add(row1)
        .map_err(|err| panic!("Adding row 1 failed: {:?}", err));

    let row2 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: TWOS_CSID,
        parents: vec![],
    };
    let test = test.and_then({
        let changesets = changesets.clone();
        move |_| {
            changesets
                .add(row2)
                .map_err(|err| panic!("Adding row 2 failed: {:?}", err))
        }
    });

    let row3 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: THREES_CSID,
        parents: vec![TWOS_CSID],
    };
    let test = test.and_then({
        let changesets = changesets.clone();
        move |_| {
            changesets
                .add(row3)
                .map_err(|err| panic!("Adding row 3 failed: {:?}", err))
        }
    });

    let row4 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: FOURS_CSID,
        parents: vec![ONES_CSID, THREES_CSID],
    };
    let test = test.and_then({
        let changesets = changesets.clone();
        move |_| {
            changesets
                .add(row4)
                .map_err(|err| panic!("Adding row 4 failed: {:?}", err))
        }
    });

    let row5 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: FIVES_CSID,
        parents: vec![ONES_CSID, TWOS_CSID, FOURS_CSID],
    };
    let test = test.and_then({
        let changesets = changesets.clone();
        move |_| {
            changesets
                .add(row5)
                .map_err(|err| panic!("Adding row 5 failed: {:?}", err))
        }
    });

    tokio::run(test);

    let test = changesets
        .get(REPO_ZERO, ONES_CSID)
        .map_err(|err| panic!("Get row 1 failed: {:?}", err))
        .map(|result| {
            assert_eq!(
                result,
                Some(ChangesetEntry {
                    repo_id: REPO_ZERO,
                    cs_id: ONES_CSID,
                    parents: vec![],
                    gen: 1,
                })
            )
        });
    tokio::run(test);

    let test = changesets
        .get(REPO_ZERO, TWOS_CSID)
        .map_err(|err| panic!("Get row 2 failed: {:?}", err))
        .map(|result| {
            assert_eq!(
                result,
                Some(ChangesetEntry {
                    repo_id: REPO_ZERO,
                    cs_id: TWOS_CSID,
                    parents: vec![],
                    gen: 1,
                })
            )
        });
    tokio::run(test);

    let test = changesets
        .get(REPO_ZERO, THREES_CSID)
        .map_err(|err| panic!("Get row 3 failed: {:?}", err))
        .map(|result| {
            assert_eq!(
                result,
                Some(ChangesetEntry {
                    repo_id: REPO_ZERO,
                    cs_id: THREES_CSID,
                    parents: vec![TWOS_CSID],
                    gen: 2,
                })
            )
        });
    tokio::run(test);

    let test = changesets
        .get(REPO_ZERO, FOURS_CSID)
        .map_err(|err| panic!("Get row 4 failed: {:?}", err))
        .map(|result| {
            assert_eq!(
                result,
                Some(ChangesetEntry {
                    repo_id: REPO_ZERO,
                    cs_id: FOURS_CSID,
                    parents: vec![ONES_CSID, THREES_CSID],
                    gen: 3,
                })
            )
        });
    tokio::run(test);

    let test = changesets
        .get(REPO_ZERO, FIVES_CSID)
        .map_err(|err| panic!("Get row 5 failed: {:?}", err))
        .map(|result| {
            assert_eq!(
                result,
                Some(ChangesetEntry {
                    repo_id: REPO_ZERO,
                    cs_id: FIVES_CSID,
                    parents: vec![ONES_CSID, TWOS_CSID, FOURS_CSID],
                    gen: 4,
                })
            )
        });
    tokio::run(test);
}

macro_rules! changesets_test_impl {
    ($mod_name: ident => {
        new: $new_cb: expr,
    }) => {
        mod $mod_name {
            use super::*;

            #[test]
            fn test_add_and_get() {
                add_and_get($new_cb());
            }

            #[test]
            fn test_add_missing_parents() {
                add_missing_parents($new_cb());
            }

            #[test]
            fn test_missing() {
                missing($new_cb());
            }

            #[test]
            fn test_duplicate() {
                duplicate($new_cb());
            }

            #[test]
            fn test_complex() {
                complex($new_cb());
            }
        }
    }
}

changesets_test_impl! {
    sqlite_test => {
        new: new_sqlite,
    }
}

changesets_test_impl! {
    sqlite_arced_test => {
        new: new_sqlite_arced,
    }
}

changesets_test_impl! {
    mysql_test => {
        new: new_mysql,
    }
}

changesets_test_impl! {
    mysql_arced_test => {
        new: new_mysql_arced,
    }
}

fn new_sqlite() -> SqliteChangesets {
    let db = SqliteChangesets::in_memory().expect("Creating an in-memory SQLite database failed");
    db
}

fn new_sqlite_arced() -> Arc<Changesets> {
    Arc::new(new_sqlite())
}

fn new_mysql() -> MysqlChangesets {
    MysqlChangesets::create_test_db("changesets_test").expect("Failed to create test database")
}

fn new_mysql_arced() -> Arc<Changesets> {
    Arc::new(new_mysql())
}
