/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Basic tests.

use std::collections::HashMap;

use anyhow::Result;
use fbinit::FacebookInit;
use maplit::{hashmap, hashset};
use mercurial_mutation::{HgMutationEntry, SqlHgMutationStoreBuilder};
use mercurial_types::HgChangesetId;
use mercurial_types_mocks::nodehash::make_hg_cs_id;
use mononoke_types_mocks::datetime::EPOCH_ZERO;
use mononoke_types_mocks::repo::REPO_ZERO;
use smallvec::smallvec;
use sql_construct::SqlConstruct;

use crate::util::check_entries;

fn create_entries() -> (
    HashMap<usize, HgMutationEntry>,
    HashMap<HgChangesetId, HgChangesetId>,
) {
    // Generate the mutation graph:
    //
    //
    //                   3  --\                                   /--> 7
    //                         \                                 /
    //   1  --(amend)->  2  --(fold)-> 4  --(rebase)->  5  --(split)-> 6
    let entries = hashmap! {
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
    };
    let primordials = hashmap! {
        make_hg_cs_id(1) => make_hg_cs_id(1),
        make_hg_cs_id(2) => make_hg_cs_id(1),
        make_hg_cs_id(3) => make_hg_cs_id(3),
        make_hg_cs_id(4) => make_hg_cs_id(1),
        make_hg_cs_id(5) => make_hg_cs_id(1),
        make_hg_cs_id(6) => make_hg_cs_id(1),
    };

    (entries, primordials)
}

#[fbinit::compat_test]
async fn fetch_predecessors(_fb: FacebookInit) -> Result<()> {
    let store = SqlHgMutationStoreBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);
    let (entries, primordials) = create_entries();
    store
        .add_raw_entries_for_test(entries.values().cloned().collect(), primordials)
        .await
        .expect("add raw entries should succeed");

    check_entries(&store, hashset![make_hg_cs_id(6)], &entries, &[2, 4, 5, 6]).await?;
    check_entries(&store, hashset![make_hg_cs_id(4)], &entries, &[2, 4]).await?;
    check_entries(&store, hashset![make_hg_cs_id(1)], &entries, &[]).await?;
    Ok(())
}
