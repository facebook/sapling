/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Basic tests.

use std::collections::HashMap;

use anyhow::Result;
use context::CoreContext;
use fbinit::FacebookInit;
use maplit::{hashmap, hashset};
use mercurial_mutation::{HgMutationEntry, HgMutationStore, SqlHgMutationStoreBuilder};
use mercurial_types_mocks::nodehash::make_hg_cs_id;
use mononoke_types_mocks::datetime::EPOCH_ZERO;
use mononoke_types_mocks::repo::REPO_ZERO;
use smallvec::smallvec;
use sql_construct::SqlConstruct;

use crate::util::check_entries;

fn create_entries() -> HashMap<usize, HgMutationEntry> {
    // Generate the mutation graph:
    //
    //
    //                   3  --.                                   .--> 7
    //                         \                                 /
    //   1  --(amend)->  2  --(fold)-> 4  --(rebase)->  5  --(split)-> 6
    hashmap! {
        2 => HgMutationEntry::new(
            make_hg_cs_id(2),
            smallvec![make_hg_cs_id(1)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            EPOCH_ZERO.clone(),
            vec![],
        ),
        4 => HgMutationEntry::new(
            make_hg_cs_id(4),
            smallvec![make_hg_cs_id(2), make_hg_cs_id(3)],
            vec![],
            String::from("fold"),
            String::from("testuser"),
            EPOCH_ZERO.clone(),
            vec![],
        ),
        5 => HgMutationEntry::new(
            make_hg_cs_id(5),
            smallvec![make_hg_cs_id(4)],
            vec![],
            String::from("rebase"),
            String::from("testuser"),
            EPOCH_ZERO.clone(),
            vec![],
        ),
        6 => HgMutationEntry::new(
            make_hg_cs_id(6),
            smallvec![make_hg_cs_id(5)],
            vec![make_hg_cs_id(7)],
            String::from("split"),
            String::from("testuser"),
            EPOCH_ZERO.clone(),
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
        smallvec![make_hg_cs_id(6)],
        vec![],
        String::from("amend"),
        String::from("testuser"),
        EPOCH_ZERO.clone(),
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
            smallvec![make_hg_cs_id(6)],
            vec![],
            String::from("rebase"),
            String::from("testuser"),
            EPOCH_ZERO.clone(),
            vec![],
        ),
        HgMutationEntry::new(
            make_hg_cs_id(10),
            smallvec![make_hg_cs_id(9)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            EPOCH_ZERO.clone(),
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
            smallvec![make_hg_cs_id(8)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            EPOCH_ZERO.clone(),
            vec![],
        ),
        HgMutationEntry::new(
            make_hg_cs_id(13),
            smallvec![
                make_hg_cs_id(10),
                make_hg_cs_id(7),
                make_hg_cs_id(11),
                make_hg_cs_id(12),
            ],
            vec![],
            String::from("combine"),
            String::from("testuser"),
            EPOCH_ZERO.clone(),
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
            smallvec![make_hg_cs_id(14)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            EPOCH_ZERO.clone(),
            vec![],
        ),
        HgMutationEntry::new(
            make_hg_cs_id(1),
            smallvec![make_hg_cs_id(15)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            EPOCH_ZERO.clone(),
            vec![],
        ),
        HgMutationEntry::new(
            make_hg_cs_id(16),
            smallvec![make_hg_cs_id(1)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            EPOCH_ZERO.clone(),
            vec![],
        ),
        HgMutationEntry::new(
            make_hg_cs_id(17),
            smallvec![make_hg_cs_id(16)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            EPOCH_ZERO.clone(),
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
