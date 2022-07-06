/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Caching tests.

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use fbinit::FacebookInit;
use maplit::hashmap;
use maplit::hashset;
use mercurial_mutation::CachedHgMutationStore;
use mercurial_mutation::HgMutationEntry;
use mercurial_mutation::HgMutationStore;
use mercurial_mutation::SqlHgMutationStoreBuilder;
use mercurial_types::HgChangesetId;
use mercurial_types_mocks::nodehash::make_hg_cs_id;
use mononoke_types::RepositoryId;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql_construct::SqlConstruct;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::basic::create_entries;
use crate::util::check_entries;

struct CountedHgMutationStore {
    inner_store: Arc<dyn HgMutationStore>,
    add_entries: Arc<AtomicUsize>,
    all_predecessors: Arc<AtomicUsize>,
}

impl CountedHgMutationStore {
    fn new(
        inner_store: Arc<dyn HgMutationStore>,
        add_entries: Arc<AtomicUsize>,
        all_predecessors: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            inner_store,
            add_entries,
            all_predecessors,
        }
    }
}

#[async_trait]
impl HgMutationStore for CountedHgMutationStore {
    fn repo_id(&self) -> RepositoryId {
        self.inner_store.repo_id()
    }

    async fn add_entries(
        &self,
        ctx: &CoreContext,
        new_changeset_ids: HashSet<HgChangesetId>,
        entries: Vec<HgMutationEntry>,
    ) -> Result<()> {
        self.add_entries.fetch_add(1, Ordering::Relaxed);
        self.inner_store
            .add_entries(ctx, new_changeset_ids, entries)
            .await
    }

    async fn all_predecessors_by_changeset(
        &self,
        ctx: &CoreContext,
        changeset_ids: HashSet<HgChangesetId>,
    ) -> Result<HashMap<HgChangesetId, Vec<HgMutationEntry>>> {
        self.all_predecessors.fetch_add(1, Ordering::Relaxed);
        self.inner_store
            .all_predecessors_by_changeset(ctx, changeset_ids)
            .await
    }
}

#[fbinit::test]
async fn add_entries_and_fetch_predecessors_with_caching(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let sql_store = SqlHgMutationStoreBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_repo_id(REPO_ZERO);

    let add_entries = Arc::new(AtomicUsize::new(0));
    let all_predecessors = Arc::new(AtomicUsize::new(0));
    let counted_store = CountedHgMutationStore::new(
        Arc::new(sql_store),
        add_entries.clone(),
        all_predecessors.clone(),
    );
    let store = CachedHgMutationStore::new_test(Arc::new(counted_store));

    // Add the initial set of entries.
    let mut entries = create_entries();
    store
        .add_entries(
            &ctx,
            hashset![make_hg_cs_id(6), make_hg_cs_id(7)],
            entries.values().cloned().collect(),
        )
        .await?;
    assert_eq!(add_entries.load(Ordering::Relaxed), 1);
    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(6)],
        &entries,
        &[2, 4, 5, 6],
    )
    .await?;
    // First time query, cache miss. Counters should go up.
    assert_eq!(all_predecessors.load(Ordering::Relaxed), 1);

    // Querying for different changeset, counters should again go up.
    check_entries(&store, &ctx, hashset![make_hg_cs_id(4)], &entries, &[2, 4]).await?;
    assert_eq!(all_predecessors.load(Ordering::Relaxed), 2);

    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(4), make_hg_cs_id(6)],
        &entries,
        &[2, 4, 5, 6],
    )
    .await?;
    // Querying for known changesets, counters should remain the same.
    assert_eq!(all_predecessors.load(Ordering::Relaxed), 2);

    // Querying for different changeset, counters should again go up.
    check_entries(&store, &ctx, hashset![make_hg_cs_id(1)], &entries, &[]).await?;
    check_entries(&store, &ctx, hashset![make_hg_cs_id(7)], &entries, &[]).await?;
    assert_eq!(all_predecessors.load(Ordering::Relaxed), 4);

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
    // Entries always bypass the cache, so the counters should always go up
    store
        .add_entries(&ctx, hashset![make_hg_cs_id(8)], vec![new_entry.clone()])
        .await?;
    assert_eq!(add_entries.load(Ordering::Relaxed), 2);
    entries.insert(8, new_entry);

    // Adding a new entry shouldn't change the result for an existing changeset.
    // The value should still be returned from cache and the counters should
    // remain the same.
    check_entries(&store, &ctx, hashset![make_hg_cs_id(7)], &entries, &[]).await?;
    assert_eq!(all_predecessors.load(Ordering::Relaxed), 4);

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

    // Entries always bypass the cache, so the counters should always go up
    store
        .add_entries(&ctx, hashset![make_hg_cs_id(10)], new_entries.clone())
        .await?;
    assert_eq!(add_entries.load(Ordering::Relaxed), 3);
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
    // The mutation history for 4 was fetched earlier, and inspite of change
    // in the primordial the history would still be served through cache.
    // Thus, instead of getting [1, 2, 4, 15] as the history we get [2, 4].
    // The new values would take place once the cache expires.
    check_entries(&store, &ctx, hashset![make_hg_cs_id(4)], &entries, &[2, 4]).await?;

    Ok(())
}

#[fbinit::test]
async fn check_mutations_are_cut_when_reaching_limit_with_caching(fb: FacebookInit) -> Result<()> {
    const TEST_MUTATION_LIMIT: usize = 10;
    let ctx = CoreContext::test_mock(fb);
    let sql_store = SqlHgMutationStoreBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_mutation_limit(TEST_MUTATION_LIMIT)
        .with_repo_id(REPO_ZERO);

    let add_entries = Arc::new(AtomicUsize::new(0));
    let all_predecessors = Arc::new(AtomicUsize::new(0));
    let counted_store = CountedHgMutationStore::new(
        Arc::new(sql_store),
        add_entries.clone(),
        all_predecessors.clone(),
    );
    let store = CachedHgMutationStore::new_test(Arc::new(counted_store));

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

    // New entry added, cache bypassed, counter updated.
    store
        .add_entries(
            &ctx,
            (1..20).map(make_hg_cs_id).collect::<HashSet<_>>(),
            new_entries.clone(),
        )
        .await?;
    assert_eq!(add_entries.load(Ordering::Relaxed), 1);
    entries.extend((1..20).zip(new_entries));

    // First we want to make sure that we are not fetching the entire history
    let fetched_entries = store
        .all_predecessors(&ctx, hashset![make_hg_cs_id(19)])
        .await?;
    assert_eq!(fetched_entries.len(), TEST_MUTATION_LIMIT);
    // First time fetch, no entry in cache, counter should go up.
    assert_eq!(all_predecessors.load(Ordering::Relaxed), 1);

    // Now we want to make sure that the mutations we are collecting, correspond to the latest amends
    check_entries(
        &store,
        &ctx,
        hashset![make_hg_cs_id(19)],
        &entries,
        &(10..20).collect::<Vec<_>>(),
    )
    .await?;
    // Now the entry is in cache, counter shouldn't move
    assert_eq!(all_predecessors.load(Ordering::Relaxed), 1);

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
