/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::ChangesetFetcher;
use super::PrefetchedChangesetsFetcher;
use anyhow::Result;
use changesets::ChangesetEntry;
use changesets::ChangesetInsert;
use changesets::Changesets;
use changesets_impl::SqlChangesetsBuilder;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream;
use mononoke_types::Generation;
use mononoke_types_mocks::changesetid::*;
use mononoke_types_mocks::repo::*;
use rendezvous::RendezVousOptions;
use sql_construct::SqlConstruct;
use std::sync::Arc;

#[fbinit::test]
async fn test_prefetched_fetcher_no_prefetching(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    // Create our mock changesets with just ONES_CSID
    let changesets = Arc::new(
        SqlChangesetsBuilder::with_sqlite_in_memory()?
            .build(RendezVousOptions::for_test(), REPO_ZERO),
    );
    let row = ChangesetInsert {
        cs_id: ONES_CSID,
        parents: vec![],
    };
    changesets.add(ctx.clone(), row).await?;

    let prefetched_fetcher =
        PrefetchedChangesetsFetcher::new(REPO_ZERO, changesets, stream::empty()).await?;

    // Confirm the generation number and parents for ONES_CSID
    assert_eq!(
        prefetched_fetcher
            .get_generation_number(ctx.clone(), ONES_CSID)
            .await?,
        Generation::new(1)
    );
    assert_eq!(
        prefetched_fetcher
            .get_parents(ctx.clone(), ONES_CSID)
            .await?,
        []
    );

    // Check that TWOS_CSID is an error
    assert!(
        prefetched_fetcher
            .get_parents(ctx, TWOS_CSID)
            .await
            .is_err()
    );

    Ok(())
}

#[fbinit::test]
async fn test_prefetched_fetcher_no_overlap(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    // Create our mock changesets with just ONES_CSID
    let changesets = Arc::new(
        SqlChangesetsBuilder::with_sqlite_in_memory()?
            .build(RendezVousOptions::for_test(), REPO_ZERO),
    );
    let row = ChangesetInsert {
        cs_id: ONES_CSID,
        parents: vec![],
    };
    changesets.add(ctx.clone(), row).await?;

    let cs2 = Ok(ChangesetEntry {
        repo_id: REPO_ZERO,
        cs_id: TWOS_CSID,
        parents: vec![ONES_CSID],
        gen: 5,
    });

    let prefetched_fetcher =
        PrefetchedChangesetsFetcher::new(REPO_ZERO, changesets, stream::iter(vec![cs2])).await?;

    // Confirm the generation number and parents for ONES_CSID
    assert_eq!(
        prefetched_fetcher
            .get_generation_number(ctx.clone(), ONES_CSID)
            .await?,
        Generation::new(1)
    );
    assert_eq!(
        prefetched_fetcher
            .get_parents(ctx.clone(), ONES_CSID)
            .await?,
        []
    );

    // Confirm the generation number and parents for TWOS_CSID
    assert_eq!(
        prefetched_fetcher
            .get_generation_number(ctx.clone(), TWOS_CSID)
            .await?,
        Generation::new(5)
    );
    assert_eq!(
        prefetched_fetcher
            .get_parents(ctx.clone(), TWOS_CSID)
            .await?,
        [ONES_CSID]
    );

    // Check that THREES_CSID is an error
    assert!(
        prefetched_fetcher
            .get_parents(ctx, THREES_CSID)
            .await
            .is_err()
    );

    Ok(())
}

#[fbinit::test]
async fn test_prefetched_fetcher_overlap(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    // Create our mock changesets with ONES_CSID and TWOS_CSID
    let changesets = Arc::new(
        SqlChangesetsBuilder::with_sqlite_in_memory()?
            .build(RendezVousOptions::for_test(), REPO_ZERO),
    );
    let row = ChangesetInsert {
        cs_id: ONES_CSID,
        parents: vec![],
    };
    changesets.add(ctx.clone(), row).await?;
    let row = ChangesetInsert {
        cs_id: TWOS_CSID,
        parents: vec![ONES_CSID],
    };
    changesets.add(ctx.clone(), row).await?;

    let cs2 = Ok(ChangesetEntry {
        repo_id: REPO_ZERO,
        cs_id: TWOS_CSID,
        parents: vec![THREES_CSID],
        gen: 5,
    });

    let prefetched_fetcher =
        PrefetchedChangesetsFetcher::new(REPO_ZERO, changesets, stream::iter(vec![cs2])).await?;

    // Confirm the generation number and parents for TWOS_CSID comes from the prefetch list
    assert_eq!(
        prefetched_fetcher
            .get_generation_number(ctx.clone(), TWOS_CSID)
            .await?,
        Generation::new(5)
    );
    assert_eq!(
        prefetched_fetcher
            .get_parents(ctx.clone(), TWOS_CSID)
            .await?,
        [THREES_CSID]
    );

    // Check that THREES_CSID is an error
    assert!(
        prefetched_fetcher
            .get_parents(ctx, THREES_CSID)
            .await
            .is_err()
    );

    Ok(())
}
