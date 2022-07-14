/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Basic tests.

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Result;
use context::CoreContext;
use fbinit::FacebookInit;
use maplit::hashmap;
use maplit::hashset;
use mercurial_mutation::HgMutationEntry;
use mercurial_mutation::HgMutationStore;
use mercurial_mutation::SqlHgMutationStoreBuilder;
use mercurial_types_mocks::nodehash::make_hg_cs_id;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql_construct::SqlConstruct;

use crate::util::check_entries;

pub(crate) fn create_entries() -> HashMap<usize, HgMutationEntry> {
    // Generate the mutation graph:
    //
    //
    //                   3  --.                                   .--> 7
    //                         \                                 /
    //   1  --(amend)->  2  --(fold)-> 4  --(rebase)->  5  --(split)-> 6
    hashmap! {
        2 => HgMutationEntry::new(
            make_hg_cs_id(2),
            vec![make_hg_cs_id(1)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        4 => HgMutationEntry::new(
            make_hg_cs_id(4),
            vec![make_hg_cs_id(2), make_hg_cs_id(3)],
            vec![],
            String::from("fold"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        5 => HgMutationEntry::new(
            make_hg_cs_id(5),
            vec![make_hg_cs_id(4)],
            vec![],
            String::from("rebase"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        6 => HgMutationEntry::new(
            make_hg_cs_id(6),
            vec![make_hg_cs_id(5)],
            vec![make_hg_cs_id(7)],
            String::from("split"),
            String::from("testuser"),
            0,
            0,
            vec![(String::from("testextra"), String::from("testextravalue"))],
        ),
    }
}

#[fbinit::test]
async fn add_entries_and_fetch_predecessors(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = SqlHgMutationStoreBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);

    // Add the initial set of entries.
    let mut entries = create_entries();
    store
        .add_entries(
            &ctx,
            hashset![make_hg_cs_id(6), make_hg_cs_id(7)],
            entries.values().cloned().collect(),
        )
        .await?;

    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(6)],
        &entries,
        &[2, 4, 5, 6],
    )
    .await?;
    check_entries(&store, &ctx, hashset![make_hg_cs_id(4)], &entries, &[2, 4]).await?;
    check_entries(&store, &ctx, hashset![make_hg_cs_id(1)], &entries, &[]).await?;
    check_entries(&store, &ctx, hashset![make_hg_cs_id(7)], &entries, &[]).await?;

    // Add one new entry.
    let new_entry = HgMutationEntry::new(
        make_hg_cs_id(8),
        vec![make_hg_cs_id(6)],
        vec![],
        String::from("amend"),
        String::from("testuser"),
        0,
        0,
        vec![],
    );
    store
        .add_entries(&ctx, hashset![make_hg_cs_id(8)], vec![new_entry.clone()])
        .await?;
    entries.insert(8, new_entry);

    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(8)],
        &entries,
        &[2, 4, 5, 6, 8],
    )
    .await?;

    // Add an entry with a gap.
    let new_entries = vec![
        HgMutationEntry::new(
            make_hg_cs_id(9),
            vec![make_hg_cs_id(6)],
            vec![],
            String::from("rebase"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        HgMutationEntry::new(
            make_hg_cs_id(10),
            vec![make_hg_cs_id(9)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
    ];

    store
        .add_entries(&ctx, hashset![make_hg_cs_id(10)], new_entries.clone())
        .await?;
    entries.extend((9..).zip(new_entries));

    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(10)],
        &entries,
        &[2, 4, 5, 6, 9, 10],
    )
    .await?;

    // Add a more complex fold with some gaps and a new primordial.  This is
    // somewhat artificial as this kind of entry is unlikely to happen in normal
    // use, however it will exercise some edge cases.
    //
    // Current graph is:
    //
    //       3 -.               .-> 7
    //           \             /
    //   1 --> 2 --> 4 -->  5 --> 6 --> 8
    //                             \
    //                              '-> 9 --> 10
    //
    // We will add:
    //
    //     10  ------------> 13
    //      7  ----------/
    //      8  --> 11 --/
    //             12 -'

    let new_entries = vec![
        HgMutationEntry::new(
            make_hg_cs_id(11),
            vec![make_hg_cs_id(8)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        HgMutationEntry::new(
            make_hg_cs_id(13),
            vec![
                make_hg_cs_id(10),
                make_hg_cs_id(7),
                make_hg_cs_id(11),
                make_hg_cs_id(12),
            ],
            vec![],
            String::from("combine"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
    ];

    store
        .add_entries(&ctx, hashset![make_hg_cs_id(13)], new_entries.clone())
        .await?;
    entries.extend(vec![11, 13].into_iter().zip(new_entries));

    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(13)],
        &entries,
        &[2, 4, 5, 6, 8, 9, 10, 11, 13],
    )
    .await?;

    // Extend history backwards.  The original client was missing some earlier
    // data.  A new client sends commits with additional history for commit 1.
    //
    //   14 --> 15 --> 1 --> 16 --> 17
    //
    // Note that the fast-path addition of mutation data won't process this,
    // but that's a reasonable trade-off.  We add two additional successors to
    // ensure we use the slow path.
    let new_entries = vec![
        HgMutationEntry::new(
            make_hg_cs_id(15),
            vec![make_hg_cs_id(14)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        HgMutationEntry::new(
            make_hg_cs_id(1),
            vec![make_hg_cs_id(15)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        HgMutationEntry::new(
            make_hg_cs_id(16),
            vec![make_hg_cs_id(1)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        HgMutationEntry::new(
            make_hg_cs_id(17),
            vec![make_hg_cs_id(16)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
    ];

    store
        .add_entries(&ctx, hashset![make_hg_cs_id(17)], new_entries.clone())
        .await?;
    entries.extend(vec![15, 1, 16, 17].into_iter().zip(new_entries));

    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(17)],
        &entries,
        &[1, 15, 16, 17],
    )
    .await?;
    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(4)],
        &entries,
        &[1, 2, 4, 15],
    )
    .await?;

    Ok(())
}

#[fbinit::test]
async fn check_mutations_are_cut_when_reaching_limit(fb: FacebookInit) -> Result<()> {
    const TEST_MUTATION_LIMIT: usize = 10;

    let ctx = CoreContext::test_mock(fb);
    let store = SqlHgMutationStoreBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_mutation_limit(TEST_MUTATION_LIMIT)
        .with_repo_id(REPO_ZERO);

    // Add a lot of entries

    let mut entries = hashmap! {};

    let mut new_entries = Vec::with_capacity(20);

    let mut amend_count: u64 = 20;

    for index in 1..amend_count {
        new_entries.push(HgMutationEntry::new(
            make_hg_cs_id(index),
            vec![make_hg_cs_id(index - 1)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ));
    }

    store
        .add_entries(
            &ctx,
            (1..20).map(make_hg_cs_id).collect::<HashSet<_>>(),
            new_entries.clone(),
        )
        .await?;

    entries.extend((1..20).zip(new_entries));

    // First we want to make sure that we are not fetching the entire history
    let fetched_entries = store
        .all_predecessors(&ctx, hashset![make_hg_cs_id(19)])
        .await?;
    assert_eq!(fetched_entries.len(), TEST_MUTATION_LIMIT);

    // Now we want to make sure that the mutations we are collecting, correspond to the latest amends
    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(19)],
        &entries,
        &(10..20).collect::<Vec<_>>(),
    )
    .await?;

    // What we want to do here is make multiple folds with 3 amends each.
    // This way, we reach the maximum number of changes (10*) BUT made specially
    // for the case where the limit cuts a fold in half. The expected behaviour is
    // for the half that wasn't cut by the limit to be removed.
    //
    // *In this case the maximum number is 10 because we are overriding it
    // in the `.with_mutation_limit(TEST_MUTATION_LIMIT)` call
    //
    // Add a lot of entries
    //
    //                             30-.
    //                                 \ <--- Here it is exceding the limit
    //                               29-.
    //                                   \
    //                                 28-.
    //                                     \
    //   19 --> 20 --> 21 --> 22 --> ... --> 27
    new_entries = Vec::with_capacity(11);

    amend_count = 7;
    // We add 7 amend operations and then create a fold that
    // will exceed the limit to check that it is actually removing
    // the mutation.
    for index in 0..amend_count {
        new_entries.push(HgMutationEntry::new(
            make_hg_cs_id(20 + index),
            vec![make_hg_cs_id(20 + index - 1)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ));
    }

    new_entries.push(HgMutationEntry::new(
        make_hg_cs_id(20 + amend_count),
        vec![
            make_hg_cs_id(20 + amend_count - 1),
            make_hg_cs_id(20 + amend_count + 1),
            make_hg_cs_id(20 + amend_count + 2),
            make_hg_cs_id(20 + amend_count + 3),
        ],
        vec![],
        String::from("combine"),
        String::from("testuser"),
        0,
        0,
        vec![],
    ));

    store
        .add_entries(
            &ctx,
            (20..32).map(make_hg_cs_id).collect::<HashSet<_>>(),
            new_entries.clone(),
        )
        .await?;

    entries.extend((20..32).zip(new_entries));

    // First we want to make sure that we are not fetching the entire history
    let fetched_entries = store
        .all_predecessors(&ctx, hashset![make_hg_cs_id(20 + amend_count)])
        .await?;

    // The last fold should be erased because it exceeds the maximum (10 in this case) and it is then
    // cut in half. So only amends stay (7)
    assert_ne!(fetched_entries.len(), 10);
    assert_eq!(fetched_entries.len(), 7);

    Ok(())
}
