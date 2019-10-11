/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Tests for the Changesets store.
use super::{
    CachingChangesets, ChangesetEntry, ChangesetInsert, Changesets, ErrorKind, SqlChangesets,
    SqlConstructors,
};
use assert_matches::assert_matches;
use caching_ext::MockStoreStats;
use context::CoreContext;
use fbinit::FacebookInit;
use maplit::hashset;
use mononoke_types_mocks::changesetid::*;
use mononoke_types_mocks::repo::*;
use std::collections::HashSet;
use std::iter::FromIterator;
use std::sync::Arc;
use tokio::runtime::Runtime;

fn run_test(fb: FacebookInit, test_fn: fn(FacebookInit, SqlChangesets)) {
    test_fn(fb, SqlChangesets::with_sqlite_in_memory().unwrap());
}

fn run_caching_test(fb: FacebookInit, test_fn: fn(FacebookInit, CachingChangesets)) {
    let real_changesets = Arc::new(SqlChangesets::with_sqlite_in_memory().unwrap());
    let changesets = CachingChangesets::mocked(real_changesets);
    test_fn(fb, changesets);
}

fn add_and_get<C: Changesets + 'static>(fb: FacebookInit, changesets: C) {
    let mut rt = Runtime::new().unwrap();
    let ctx = CoreContext::test_mock(fb);

    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };

    rt.block_on(changesets.add(ctx.clone(), row))
        .expect("Adding new entry failed");

    let result = rt
        .block_on(changesets.get(ctx, REPO_ZERO, ONES_CSID))
        .expect("Get failed");

    assert_eq!(
        result,
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: ONES_CSID,
            parents: vec![],
            gen: 1,
        }),
    );
}

fn add_missing_parents<C: Changesets>(fb: FacebookInit, changesets: C) {
    let mut rt = Runtime::new().unwrap();
    let ctx = CoreContext::test_mock(fb);

    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![TWOS_CSID],
    };

    let result = rt
        .block_on(changesets.add(ctx, row))
        .expect_err("Adding entry with missing parents failed (should have succeeded)");

    assert_matches!(
        result.downcast::<ErrorKind>(),
        Ok(ErrorKind::MissingParents(ref x)) if x == &vec![TWOS_CSID]
    );
}

fn missing<C: Changesets + 'static>(fb: FacebookInit, changesets: C) {
    let mut rt = Runtime::new().unwrap();
    let ctx = CoreContext::test_mock(fb);

    let result = rt
        .block_on(changesets.get(ctx, REPO_ZERO, ONES_CSID))
        .expect("Failed to fetch missing changeset (should succeed with None instead)");

    assert_eq!(result, None);
}

fn duplicate<C: Changesets + 'static>(fb: FacebookInit, changesets: C) {
    let mut rt = Runtime::new().unwrap();
    let ctx = CoreContext::test_mock(fb);

    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };

    assert_eq!(
        rt.block_on(changesets.add(ctx.clone(), row.clone()))
            .expect("Adding new entry failed"),
        true,
        "inserting unique changeset must return true"
    );

    assert_eq!(
        rt.block_on(changesets.add(ctx.clone(), row.clone()))
            .expect("error while adding changeset"),
        false,
        "inserting the same changeset must return false"
    );
}

fn broken_duplicate<C: Changesets + 'static>(fb: FacebookInit, changesets: C) {
    let mut rt = Runtime::new().unwrap();
    let ctx = CoreContext::test_mock(fb);

    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };
    assert_eq!(
        rt.block_on(changesets.add(ctx.clone(), row))
            .expect("Adding new entry failed"),
        true,
        "inserting unique changeset must return true"
    );

    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: TWOS_CSID,
        parents: vec![],
    };
    assert_eq!(
        rt.block_on(changesets.add(ctx.clone(), row))
            .expect("Adding new entry failed"),
        true,
        "inserting unique changeset must return true"
    );

    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![TWOS_CSID],
    };
    let result = rt
        .block_on(changesets.add(ctx.clone(), row))
        .expect_err("Adding changeset with the same hash but differen parents should fail");
    match result.downcast::<ErrorKind>() {
        Ok(ErrorKind::DuplicateInsertionInconsistency(..)) => {}
        err => panic!("unexpected error: {:?}", err),
    };
}

fn complex<C: Changesets>(fb: FacebookInit, changesets: C) {
    let mut rt = Runtime::new().unwrap();
    let ctx = CoreContext::test_mock(fb);

    let row1 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };
    rt.block_on(changesets.add(ctx.clone(), row1))
        .expect("Adding row 1 failed");

    let row2 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: TWOS_CSID,
        parents: vec![],
    };
    rt.block_on(changesets.add(ctx.clone(), row2))
        .expect("Adding row 2 failed");

    let row3 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: THREES_CSID,
        parents: vec![TWOS_CSID],
    };
    rt.block_on(changesets.add(ctx.clone(), row3))
        .expect("Adding row 3 failed");

    let row4 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: FOURS_CSID,
        parents: vec![ONES_CSID, THREES_CSID],
    };
    rt.block_on(changesets.add(ctx.clone(), row4))
        .expect("Adding row 4 failed");

    let row5 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: FIVES_CSID,
        parents: vec![ONES_CSID, TWOS_CSID, FOURS_CSID],
    };
    rt.block_on(changesets.add(ctx.clone(), row5))
        .expect("Adding row 5 failed");

    assert_eq!(
        rt.block_on(changesets.get(ctx.clone(), REPO_ZERO, ONES_CSID))
            .expect("Get row 1 failed"),
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: ONES_CSID,
            parents: vec![],
            gen: 1,
        }),
    );

    assert_eq!(
        rt.block_on(changesets.get(ctx.clone(), REPO_ZERO, TWOS_CSID))
            .expect("Get row 2 failed"),
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: TWOS_CSID,
            parents: vec![],
            gen: 1,
        }),
    );

    assert_eq!(
        rt.block_on(changesets.get(ctx.clone(), REPO_ZERO, THREES_CSID))
            .expect("Get row 3 failed"),
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: THREES_CSID,
            parents: vec![TWOS_CSID],
            gen: 2,
        }),
    );

    assert_eq!(
        rt.block_on(changesets.get(ctx.clone(), REPO_ZERO, FOURS_CSID))
            .expect("Get row 4 failed"),
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: FOURS_CSID,
            parents: vec![ONES_CSID, THREES_CSID],
            gen: 3,
        }),
    );

    assert_eq!(
        rt.block_on(changesets.get(ctx.clone(), REPO_ZERO, FIVES_CSID))
            .expect("Get row 5 failed"),
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: FIVES_CSID,
            parents: vec![ONES_CSID, TWOS_CSID, FOURS_CSID],
            gen: 4,
        }),
    );
}

fn get_many<C: Changesets>(fb: FacebookInit, changesets: C) {
    let mut rt = Runtime::new().unwrap();
    let ctx = CoreContext::test_mock(fb);

    let row1 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };
    rt.block_on(changesets.add(ctx.clone(), row1))
        .expect("Adding row 1 failed");

    let row2 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: TWOS_CSID,
        parents: vec![],
    };
    rt.block_on(changesets.add(ctx.clone(), row2))
        .expect("Adding row 2 failed");

    let row3 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: THREES_CSID,
        parents: vec![TWOS_CSID],
    };
    rt.block_on(changesets.add(ctx.clone(), row3))
        .expect("Adding row 3 failed");

    let row4 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: FOURS_CSID,
        parents: vec![ONES_CSID, THREES_CSID],
    };
    rt.block_on(changesets.add(ctx.clone(), row4))
        .expect("Adding row 4 failed");

    let row5 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: FIVES_CSID,
        parents: vec![THREES_CSID, ONES_CSID, TWOS_CSID, FOURS_CSID],
    };
    rt.block_on(changesets.add(ctx.clone(), row5))
        .expect("Adding row 5 failed");

    let actual = rt
        .block_on(changesets.get_many(ctx.clone(), REPO_ZERO, vec![ONES_CSID, TWOS_CSID]))
        .expect("Get row 1 failed");

    assert_eq!(
        HashSet::from_iter(actual),
        hashset![
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: ONES_CSID,
                parents: vec![],
                gen: 1,
            },
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: TWOS_CSID,
                parents: vec![],
                gen: 1,
            },
        ]
    );

    let actual = rt
        .block_on(changesets.get_many(
            ctx.clone(),
            REPO_ZERO,
            vec![ONES_CSID, TWOS_CSID, THREES_CSID],
        ))
        .expect("Get row 2 failed");

    assert_eq!(
        HashSet::from_iter(actual),
        hashset![
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: ONES_CSID,
                parents: vec![],
                gen: 1,
            },
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: TWOS_CSID,
                parents: vec![],
                gen: 1,
            },
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: THREES_CSID,
                parents: vec![TWOS_CSID],
                gen: 2,
            },
        ]
    );

    let actual = rt
        .block_on(changesets.get_many(ctx.clone(), REPO_ZERO, vec![]))
        .expect("Get row 3 failed");

    assert_eq!(HashSet::from_iter(actual), hashset![]);

    let actual = rt
        .block_on(changesets.get_many(ctx.clone(), REPO_ZERO, vec![ONES_CSID, FOURS_CSID]))
        .expect("Get row 2 failed");

    assert_eq!(
        HashSet::from_iter(actual),
        hashset![
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: ONES_CSID,
                parents: vec![],
                gen: 1,
            },
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: FOURS_CSID,
                parents: vec![ONES_CSID, THREES_CSID],
                gen: 3,
            },
        ]
    );

    let actual = rt
        .block_on(changesets.get_many(
            ctx.clone(),
            REPO_ZERO,
            vec![ONES_CSID, FOURS_CSID, FIVES_CSID],
        ))
        .expect("Get row 2 failed");

    assert_eq!(
        HashSet::from_iter(actual),
        hashset![
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: ONES_CSID,
                parents: vec![],
                gen: 1,
            },
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: FOURS_CSID,
                parents: vec![ONES_CSID, THREES_CSID],
                gen: 3,
            },
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: FIVES_CSID,
                parents: vec![THREES_CSID, ONES_CSID, TWOS_CSID, FOURS_CSID],
                gen: 4,
            },
        ]
    );
}

fn get_many_missing<C: Changesets>(fb: FacebookInit, changesets: C) {
    let mut rt = Runtime::new().unwrap();
    let ctx = CoreContext::test_mock(fb);

    let row1 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };
    rt.block_on(changesets.add(ctx.clone(), row1))
        .expect("Adding row 1 failed");

    let row2 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: TWOS_CSID,
        parents: vec![],
    };
    rt.block_on(changesets.add(ctx.clone(), row2))
        .expect("Adding row 2 failed");

    let actual = rt
        .block_on(changesets.get_many(
            ctx.clone(),
            REPO_ZERO,
            vec![ONES_CSID, TWOS_CSID, THREES_CSID],
        ))
        .expect("get_many failed");

    assert_eq!(
        HashSet::from_iter(actual),
        hashset![
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: ONES_CSID,
                parents: vec![],
                gen: 1,
            },
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: TWOS_CSID,
                parents: vec![],
                gen: 1,
            },
        ]
    );
}

fn caching_fill<C: Changesets + 'static>(fb: FacebookInit, changesets: C) {
    let mut rt = Runtime::new().unwrap();

    let changesets = Arc::new(changesets);
    let mut cc = CachingChangesets::mocked(changesets.clone());

    let ctx = CoreContext::test_mock(fb);

    let row1 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };

    let row2 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: TWOS_CSID,
        parents: vec![],
    };

    let row3 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: THREES_CSID,
        parents: vec![],
    };

    rt.block_on(changesets.add(ctx.clone(), row1))
        .expect("Adding row 1 failed");

    rt.block_on(changesets.add(ctx.clone(), row2))
        .expect("Adding row 2 failed");

    rt.block_on(changesets.add(ctx.clone(), row3))
        .expect("Adding row 3 failed");

    // First read should miss everyhwere.
    let _ = rt
        .block_on(cc.get_many(ctx.clone(), REPO_ZERO, vec![ONES_CSID, TWOS_CSID]))
        .expect("read 1 failed");

    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 2,
            sets: 2,
            misses: 2,
            hits: 0
        },
        "read 1, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 2,
            sets: 2,
            misses: 2,
            hits: 0
        },
        "read 1, memcache"
    );

    // Another read with the same pool should hit in local cache.
    let _ = rt
        .block_on(cc.get_many(ctx.clone(), REPO_ZERO, vec![ONES_CSID, TWOS_CSID]))
        .expect("read 2 failed");

    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 4,
            sets: 2,
            misses: 2,
            hits: 2
        },
        "read 2, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 2,
            sets: 2,
            misses: 2,
            hits: 0
        },
        "read 2, memcache"
    );

    cc = cc.fork_cachelib();

    // Read with a separate pool should hit in Memcache.
    let _ = rt
        .block_on(cc.get_many(ctx.clone(), REPO_ZERO, vec![ONES_CSID, TWOS_CSID]))
        .expect("read 3 failed");

    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 2,
            sets: 2,
            misses: 2,
            hits: 0
        },
        "read 3, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 4,
            sets: 2,
            misses: 2,
            hits: 2
        },
        "read 3, memcache"
    );

    // Reading again from the separate pool should now hit in local cache.
    let _ = rt
        .block_on(cc.get_many(ctx.clone(), REPO_ZERO, vec![ONES_CSID, TWOS_CSID]))
        .expect("read 4 failed");

    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 4,
            sets: 2,
            misses: 2,
            hits: 2
        },
        "read 4, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 4,
            sets: 2,
            misses: 2,
            hits: 2
        },
        "read 4, memcache"
    );

    // Partial read should partially hit
    let _ = rt
        .block_on(cc.get_many(
            ctx.clone(),
            REPO_ZERO,
            vec![ONES_CSID, TWOS_CSID, THREES_CSID],
        ))
        .expect("read 5 failed");

    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 7,
            sets: 3,
            misses: 3,
            hits: 4
        },
        "read 5, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 5,
            sets: 3,
            misses: 3,
            hits: 2
        },
        "read 5, memcache"
    );

    // Partial read should have filled local cache.
    let _ = rt
        .block_on(cc.get_many(
            ctx.clone(),
            REPO_ZERO,
            vec![ONES_CSID, TWOS_CSID, THREES_CSID],
        ))
        .expect("read 6 failed");

    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 10,
            sets: 3,
            misses: 3,
            hits: 7
        },
        "read 6, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 5,
            sets: 3,
            misses: 3,
            hits: 2
        },
        "read 6, memcache"
    );
}

fn caching_shared<C: Changesets + 'static>(fb: FacebookInit, changesets: C) {
    let mut rt = Runtime::new().unwrap();

    let changesets = Arc::new(changesets);
    let cc = CachingChangesets::mocked(changesets.clone());

    let ctx = CoreContext::test_mock(fb);

    let row1 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };

    let row2 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: TWOS_CSID,
        parents: vec![],
    };

    let row3 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: THREES_CSID,
        parents: vec![],
    };

    rt.block_on(changesets.add(ctx.clone(), row1))
        .expect("Adding row 1 failed");

    rt.block_on(changesets.add(ctx.clone(), row2))
        .expect("Adding row 2 failed");

    rt.block_on(changesets.add(ctx.clone(), row3))
        .expect("Adding row 3 failed");

    // get should miss
    let _ = rt
        .block_on(cc.get(ctx.clone(), REPO_ZERO, ONES_CSID))
        .expect("read 1 failed");

    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 1,
            sets: 1,
            misses: 1,
            hits: 0
        },
        "read 1, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 1,
            sets: 1,
            misses: 1,
            hits: 0
        },
        "read 1, memcache"
    );

    // get_many should hit for what was filled by get
    let _ = rt
        .block_on(cc.get_many(
            ctx.clone(),
            REPO_ZERO,
            vec![ONES_CSID, TWOS_CSID, THREES_CSID],
        ))
        .expect("read 2 failed");

    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 4,
            sets: 3,
            misses: 3,
            hits: 1
        },
        "read 2, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 3,
            sets: 3,
            misses: 3,
            hits: 0
        },
        "read 2, memcache"
    );

    // get should hit for what was filled by get_many
    let _ = rt
        .block_on(cc.get(ctx.clone(), REPO_ZERO, THREES_CSID))
        .expect("read 3 failed");

    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 5,
            sets: 3,
            misses: 3,
            hits: 2
        },
        "read 3, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 3,
            sets: 3,
            misses: 3,
            hits: 0
        },
        "read 3, memcache"
    );
}

// NOTE: Use this wrapper macro to make sure tests are executed both with Changesets and
// CachingChangesets. Define tests using #[test] if you need to only execute them for Changesets or
// CachingChangesets.
macro_rules! testify {
    ($plain_name: ident, $caching_name: ident, $input: ident) => {
        #[fbinit::test]
        fn $plain_name(fb: FacebookInit) {
            run_test(fb, $input);
        }

        #[fbinit::test]
        fn $caching_name(fb: FacebookInit) {
            run_caching_test(fb, $input);
        }
    };
}

testify!(test_add_and_get, test_caching_add_and_get, add_and_get);
testify!(
    test_add_missing_parents,
    test_caching_add_missing_parents,
    add_missing_parents
);
testify!(test_missing, test_caching_missing, missing);
testify!(test_duplicate, test_caching_duplicate, duplicate);
testify!(
    test_broken_duplicate,
    test_caching_broken_duplicate,
    broken_duplicate
);
testify!(test_complex, test_caching_complex, complex);
testify!(test_get_many, test_caching_get_many, get_many);
testify!(
    test_get_many_missing,
    test_caching_get_many_missing,
    get_many_missing
);

#[fbinit::test]
fn test_caching_fill(fb: FacebookInit) {
    run_test(fb, caching_fill);
}

#[fbinit::test]
fn test_caching_shared(fb: FacebookInit) {
    run_test(fb, caching_shared);
}
