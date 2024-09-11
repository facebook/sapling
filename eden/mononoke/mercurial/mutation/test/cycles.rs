/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests involving cycles.
//!
//! Ordinarily, cycles in mutation data is invalid, however they can occur
//! with synthetic mutation data backfilled from obsmarkers.
//!
//! The mutation store drops self-referential entries (where the successor
//! is one of the predecessors).  These will have been converted from revive
//! obsmarkers.
//!
//! If a cycle is formed within the store itself (via multiple additions that
//! eventually form a cycle), this is not a problem for the store.  Any
//! request for a commit within that cycle will return the full cycle, and
//! client-side cycle-breaking will resolve the issue.

use std::collections::HashMap;

use anyhow::Result;
use context::CoreContext;
use fbinit::FacebookInit;
use maplit::hashmap;
use maplit::hashset;
use mercurial_mutation::HgMutationEntry;
use mercurial_mutation::HgMutationStore;
use mercurial_mutation::SqlHgMutationStoreBuilder;
use mercurial_types_mocks::nodehash::make_hg_cs_id;
use mononoke_macros::mononoke;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql_construct::SqlConstruct;

use crate::util::check_entries;
use crate::util::get_entries;

fn create_entries() -> HashMap<usize, HgMutationEntry> {
    // Generate the mutation graph:
    //
    // Self-cycle:
    //
    //   1  --(amend)-> 2
    //  / \
    //  \_/ (revive)
    //
    // Simple cycle:
    //
    //   3  --(amend)--> 4 --(rebase)--> 5 --(undo)--> 3
    //
    // Cycle through second predecessor:
    //
    //   6  --(amend)--> 7 --(fold)--> 8 --(unfold)--> 9
    //                       /
    //                   9 -'
    //
    // Second predecessor is in a separate cycle:
    //
    //                  10 --(amend)--> 11 --(fold)--> 14 --(rebase)--> 15
    //                                       /
    //                    12 --(amend)-->  13 --(undo)--> 12
    //
    hashmap! {
        1 => HgMutationEntry::new(
            make_hg_cs_id(1),
            vec![make_hg_cs_id(1)],
            vec![],
            String::from("revive"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
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
        3 => HgMutationEntry::new(
            make_hg_cs_id(3),
            vec![make_hg_cs_id(5)],
            vec![],
            String::from("undo"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        4 => HgMutationEntry::new(
            make_hg_cs_id(4),
            vec![make_hg_cs_id(3)],
            vec![],
            String::from("amend"),
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
        7 => HgMutationEntry::new(
            make_hg_cs_id(7),
            vec![make_hg_cs_id(6)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        8 => HgMutationEntry::new(
            make_hg_cs_id(8),
            vec![make_hg_cs_id(7), make_hg_cs_id(9)],
            vec![],
            String::from("fold"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        9 => HgMutationEntry::new(
            make_hg_cs_id(9),
            vec![make_hg_cs_id(8)],
            vec![],
            String::from("unfold"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        11 => HgMutationEntry::new(
            make_hg_cs_id(11),
            vec![make_hg_cs_id(10)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        12 => HgMutationEntry::new(
            make_hg_cs_id(12),
            vec![make_hg_cs_id(13)],
            vec![],
            String::from("undo"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        13 => HgMutationEntry::new(
            make_hg_cs_id(13),
            vec![make_hg_cs_id(12)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        14 => HgMutationEntry::new(
            make_hg_cs_id(14),
            vec![make_hg_cs_id(11), make_hg_cs_id(13)],
            vec![],
            String::from("fold"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        15 => HgMutationEntry::new(
            make_hg_cs_id(15),
            vec![make_hg_cs_id(14)],
            vec![],
            String::from("rebase"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
    }
}

#[mononoke::fbinit_test]
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
            hashset![
                make_hg_cs_id(2),
                make_hg_cs_id(5),
                make_hg_cs_id(9),
                make_hg_cs_id(15)
            ],
            entries.values().cloned().collect(),
        )
        .await?;

    // The entry for 1 was ignored as it was self-referential.  The entry for 2
    // was stored.
    check_entries(&store, &ctx, hashset![make_hg_cs_id(2)], &entries, &[2]).await?;

    // The entries for 3, 4 and 5 were stored even though they are a loop.
    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(5)],
        &entries,
        &[3, 4, 5],
    )
    .await?;

    // The entries for 7, 8 and 9 were stored ok.
    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(9)],
        &entries,
        &[7, 8, 9],
    )
    .await?;

    // The history for 15 includes the full cycle.
    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(15)],
        &entries,
        &[11, 12, 13, 14, 15],
    )
    .await?;

    // A different client pushes without the cycle, but it's ignored as it was
    // not first.
    store
        .add_entries(
            &ctx,
            hashset![make_hg_cs_id(14)],
            get_entries(&entries, &[11, 13, 14]),
        )
        .await?;

    // The history for 15 still has the cycle.
    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(15)],
        &entries,
        &[11, 12, 13, 14, 15],
    )
    .await?;

    // Another client sends entries that create a cycle with existing entries.
    let new_entries = vec![
        HgMutationEntry::new(
            make_hg_cs_id(16),
            vec![make_hg_cs_id(2)],
            vec![],
            String::from("amend"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
        HgMutationEntry::new(
            make_hg_cs_id(1),
            vec![make_hg_cs_id(16)],
            vec![],
            String::from("undo"),
            String::from("testuser"),
            0,
            0,
            vec![],
        ),
    ];

    store
        .add_entries(&ctx, hashset![make_hg_cs_id(1)], new_entries.clone())
        .await?;
    entries.extend(vec![16, 1].into_iter().zip(new_entries));

    // The replacement entry for 1 was accepted and now shows the cycle.
    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(2)],
        &entries,
        &[1, 2, 16],
    )
    .await?;

    // The entry for 16 was also accepted and shows the cycle, too.
    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(16)],
        &entries,
        &[1, 2, 16],
    )
    .await?;

    Ok(())
}
