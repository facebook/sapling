/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests for the synced commits mapping.

use std::sync::Arc;

use anyhow::Error;
use caching_ext::CacheHandlerFactory;
use context::CoreContext;
use fbinit::FacebookInit;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_macros::mononoke;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::REPO_ONE;
use mononoke_types_mocks::repo::REPO_ZERO;
use rendezvous::RendezVousOptions;
use sql_construct::SqlConstruct;
use synced_commit_mapping::CachingSyncedCommitMapping;
use synced_commit_mapping::EquivalentWorkingCopyEntry;
use synced_commit_mapping::FetchedMappingEntry;
use synced_commit_mapping::SqlSyncedCommitMapping;
use synced_commit_mapping::SqlSyncedCommitMappingBuilder;
use synced_commit_mapping::SyncedCommitMapping;
use synced_commit_mapping::SyncedCommitMappingEntry;
use synced_commit_mapping::SyncedCommitSourceRepo;
use synced_commit_mapping::WorkingCopyEquivalence;

fn sql_synced_commit_mapping() -> SqlSyncedCommitMapping {
    SqlSyncedCommitMappingBuilder::with_sqlite_in_memory()
        .unwrap()
        .build(RendezVousOptions::for_test())
}

fn caching_synced_commit_mapping() -> CachingSyncedCommitMapping {
    CachingSyncedCommitMapping::new(
        Arc::new(sql_synced_commit_mapping()),
        CacheHandlerFactory::Mocked,
    )
    .unwrap()
}

#[mononoke::fbinit_test]
async fn test_get_many_no_mappings_sql(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let synced_commit_mapping = sql_synced_commit_mapping();
    let result = synced_commit_mapping
        .get_many(&ctx, REPO_ZERO, REPO_ONE, &[bonsai::ONES_CSID])
        .await
        .expect("Get failed");
    assert!(result.contains_key(&bonsai::ONES_CSID));
}

#[mononoke::fbinit_test]
async fn test_get_many_no_mappings_caching(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let synced_commit_mapping = caching_synced_commit_mapping();
    let result = synced_commit_mapping
        .get_many(&ctx, REPO_ZERO, REPO_ONE, &[bonsai::ONES_CSID])
        .await
        .expect("Get failed");
    assert!(result.contains_key(&bonsai::ONES_CSID));
}

#[mononoke::fbinit_test]
async fn test_add_and_get_sql(fb: FacebookInit) {
    test_add_and_get_impl(fb, sql_synced_commit_mapping()).await;
}

#[mononoke::fbinit_test]
async fn test_add_and_get_caching(fb: FacebookInit) {
    test_add_and_get_impl(fb, caching_synced_commit_mapping()).await;
}

async fn test_add_and_get_impl(fb: FacebookInit, mapping: impl SyncedCommitMapping) {
    let version_name = CommitSyncConfigVersion("TEST_VERSION_NAME".to_string());
    let ctx = CoreContext::test_mock(fb);
    let entry = SyncedCommitMappingEntry::new(
        REPO_ZERO,
        bonsai::ONES_CSID,
        REPO_ONE,
        bonsai::TWOS_CSID,
        version_name.clone(),
        SyncedCommitSourceRepo::Large,
    );
    assert!(
        mapping
            .add(&ctx, entry.clone())
            .await
            .expect("Adding new entry failed")
    );
    assert!(
        !mapping
            .add(&ctx, entry)
            .await
            .expect("Adding same entry failed")
    );

    let res = mapping
        .get_equivalent_working_copy(&ctx, REPO_ZERO, bonsai::ONES_CSID, REPO_ONE)
        .await
        .expect("get equivalent wc failed, should succeed");

    assert_eq!(
        res,
        Some(WorkingCopyEquivalence::WorkingCopy(
            bonsai::TWOS_CSID,
            version_name.clone(),
        ))
    );

    // insert again
    let entry = SyncedCommitMappingEntry::new(
        REPO_ZERO,
        bonsai::THREES_CSID,
        REPO_ONE,
        bonsai::FOURS_CSID,
        version_name.clone(),
        SyncedCommitSourceRepo::Large,
    );
    assert!(
        mapping
            .add(&ctx, entry.clone())
            .await
            .expect("Adding new entry failed")
    );

    let res = mapping
        .get_equivalent_working_copy(&ctx, REPO_ZERO, bonsai::THREES_CSID, REPO_ONE)
        .await
        .expect("get equivalent wc failed, should succeed");

    assert_eq!(
        res,
        Some(WorkingCopyEquivalence::WorkingCopy(
            bonsai::FOURS_CSID,
            version_name.clone(),
        ))
    );

    let result = mapping
        .get(&ctx, REPO_ZERO, bonsai::ONES_CSID, REPO_ONE)
        .await
        .expect("Get failed")
        .into_iter()
        .next()
        .expect("Unexpectedly, mapping is absent");
    let result_maybe_stale = mapping
        .get_maybe_stale(&ctx, REPO_ZERO, bonsai::ONES_CSID, REPO_ONE)
        .await
        .expect("Get failed")
        .into_iter()
        .next()
        .expect("Unexpectedly, mapping is absent");
    assert_eq!(
        result,
        (
            bonsai::TWOS_CSID,
            Some(version_name.clone()),
            Some(SyncedCommitSourceRepo::Large)
        )
    );
    assert_eq!(
        result_maybe_stale,
        FetchedMappingEntry {
            target_bcs_id: bonsai::TWOS_CSID,
            maybe_version_name: Some(version_name.clone()),
            maybe_source_repo: Some(SyncedCommitSourceRepo::Large)
        }
    );
    let result = mapping
        .get(&ctx, REPO_ONE, bonsai::TWOS_CSID, REPO_ZERO)
        .await
        .expect("Get failed")
        .into_iter()
        .next()
        .expect("Unexpectedly, mapping is absent");
    let result_maybe_stale = mapping
        .get_maybe_stale(&ctx, REPO_ONE, bonsai::TWOS_CSID, REPO_ZERO)
        .await
        .expect("Get failed")
        .into_iter()
        .next()
        .expect("Unexpectedly, mapping is absent");
    assert_eq!(
        result,
        (
            bonsai::ONES_CSID,
            Some(version_name.clone()),
            Some(SyncedCommitSourceRepo::Large)
        )
    );
    assert_eq!(
        result_maybe_stale,
        FetchedMappingEntry {
            target_bcs_id: bonsai::ONES_CSID,
            maybe_version_name: Some(version_name.clone()),
            maybe_source_repo: Some(SyncedCommitSourceRepo::Large)
        }
    );

    let result = mapping
        .get_many(
            &ctx,
            REPO_ZERO,
            REPO_ONE,
            &[bonsai::ONES_CSID, bonsai::THREES_CSID],
        )
        .await
        .expect("Get many failed");
    let result_maybe_stale = mapping
        .get_many_maybe_stale(
            &ctx,
            REPO_ZERO,
            REPO_ONE,
            &[bonsai::ONES_CSID, bonsai::THREES_CSID],
        )
        .await
        .expect("Get many maybe stale failed");
    assert_eq!(
        result
            .get(&bonsai::ONES_CSID)
            .unwrap()
            .iter()
            .next()
            .expect("Unexpectedly, mapping is absent"),
        &FetchedMappingEntry {
            target_bcs_id: bonsai::TWOS_CSID,
            maybe_version_name: Some(version_name.clone()),
            maybe_source_repo: Some(SyncedCommitSourceRepo::Large)
        }
    );
    assert_eq!(
        result
            .get(&bonsai::THREES_CSID)
            .unwrap()
            .iter()
            .next()
            .expect("Unexpectedly, mapping is absent"),
        &FetchedMappingEntry {
            target_bcs_id: bonsai::FOURS_CSID,
            maybe_version_name: Some(version_name.clone()),
            maybe_source_repo: Some(SyncedCommitSourceRepo::Large),
        }
    );
    assert_eq!(
        result_maybe_stale
            .get(&bonsai::ONES_CSID)
            .unwrap()
            .iter()
            .next()
            .expect("Unexpectedly, mapping is absent"),
        &FetchedMappingEntry {
            target_bcs_id: bonsai::TWOS_CSID,
            maybe_version_name: Some(version_name.clone()),
            maybe_source_repo: Some(SyncedCommitSourceRepo::Large)
        }
    );
    assert_eq!(
        result_maybe_stale
            .get(&bonsai::THREES_CSID)
            .unwrap()
            .iter()
            .next()
            .expect("Unexpectedly, mapping is absent"),
        &FetchedMappingEntry {
            target_bcs_id: bonsai::FOURS_CSID,
            maybe_version_name: Some(version_name.clone()),
            maybe_source_repo: Some(SyncedCommitSourceRepo::Large),
        }
    );

    let result = mapping
        .get_many(
            &ctx,
            REPO_ONE,
            REPO_ZERO,
            &[bonsai::FOURS_CSID, bonsai::TWOS_CSID],
        )
        .await
        .expect("Get many failed");
    let result_maybe_stale = mapping
        .get_many_maybe_stale(
            &ctx,
            REPO_ONE,
            REPO_ZERO,
            &[bonsai::FOURS_CSID, bonsai::TWOS_CSID],
        )
        .await
        .expect("Get many maybe stale failed");
    assert_eq!(
        result
            .get(&bonsai::TWOS_CSID)
            .unwrap()
            .iter()
            .next()
            .expect("Unexpectedly, mapping is absent"),
        &FetchedMappingEntry {
            target_bcs_id: bonsai::ONES_CSID,
            maybe_version_name: Some(version_name.clone()),
            maybe_source_repo: Some(SyncedCommitSourceRepo::Large)
        }
    );
    assert_eq!(
        result
            .get(&bonsai::FOURS_CSID)
            .unwrap()
            .iter()
            .next()
            .expect("Unexpectedly, mapping is absent"),
        &FetchedMappingEntry {
            target_bcs_id: bonsai::THREES_CSID,
            maybe_version_name: Some(version_name.clone()),
            maybe_source_repo: Some(SyncedCommitSourceRepo::Large)
        }
    );
    assert_eq!(
        result_maybe_stale
            .get(&bonsai::TWOS_CSID)
            .unwrap()
            .iter()
            .next()
            .expect("Unexpectedly, mapping is absent"),
        &FetchedMappingEntry {
            target_bcs_id: bonsai::ONES_CSID,
            maybe_version_name: Some(version_name.clone()),
            maybe_source_repo: Some(SyncedCommitSourceRepo::Large)
        }
    );
    assert_eq!(
        result_maybe_stale
            .get(&bonsai::FOURS_CSID)
            .unwrap()
            .iter()
            .next()
            .expect("Unexpectedly, mapping is absent"),
        &FetchedMappingEntry {
            target_bcs_id: bonsai::THREES_CSID,
            maybe_version_name: Some(version_name.clone()),
            maybe_source_repo: Some(SyncedCommitSourceRepo::Large)
        }
    );
}

#[mononoke::fbinit_test]
async fn test_missing_sql(fb: FacebookInit) {
    test_missing_impl(fb, sql_synced_commit_mapping()).await
}

#[mononoke::fbinit_test]
async fn test_missing_caching(fb: FacebookInit) {
    test_missing_impl(fb, caching_synced_commit_mapping()).await
}

async fn test_missing_impl(fb: FacebookInit, mapping: impl SyncedCommitMapping) {
    let ctx = CoreContext::test_mock(fb);
    let result = mapping
        .get(&ctx, REPO_ONE, bonsai::TWOS_CSID, REPO_ZERO)
        .await
        .expect("Failed to fetch missing changeset (should succeed with None instead)");
    assert!(result.is_empty());
}

#[mononoke::fbinit_test]
async fn test_equivalent_working_copy_sql(fb: FacebookInit) {
    test_equivalent_working_copy_impl(fb, sql_synced_commit_mapping()).await
}

#[mononoke::fbinit_test]
async fn test_equivalent_working_copy_caching(fb: FacebookInit) {
    test_equivalent_working_copy_impl(fb, caching_synced_commit_mapping()).await
}

async fn test_equivalent_working_copy_impl(fb: FacebookInit, mapping: impl SyncedCommitMapping) {
    let ctx = CoreContext::test_mock(fb);
    let version_name = CommitSyncConfigVersion("TEST_VERSION_NAME".to_string());
    let result = mapping
        .get_equivalent_working_copy(&ctx, REPO_ONE, bonsai::TWOS_CSID, REPO_ZERO)
        .await
        .expect("Failed to fetch equivalent working copy (should succeed with None instead)");
    assert_eq!(result, None);

    let entry = EquivalentWorkingCopyEntry {
        large_repo_id: REPO_ZERO,
        large_bcs_id: bonsai::ONES_CSID,
        small_repo_id: REPO_ONE,
        small_bcs_id: Some(bonsai::TWOS_CSID),
        version_name: Some(version_name.clone()),
    };
    let result = mapping
        .insert_equivalent_working_copy(&ctx, entry.clone())
        .await
        .expect("Failed to insert working copy");
    assert!(result);

    let result = mapping
        .insert_equivalent_working_copy(&ctx, entry)
        .await
        .expect("Failed to insert working copy");
    assert!(!result);

    let res = mapping
        .get_equivalent_working_copy(&ctx, REPO_ZERO, bonsai::ONES_CSID, REPO_ONE)
        .await
        .expect("get equivalent wc failed, should succeed");

    assert_eq!(
        res,
        Some(WorkingCopyEquivalence::WorkingCopy(
            bonsai::TWOS_CSID,
            version_name
        ))
    );

    let null_entry = EquivalentWorkingCopyEntry {
        large_repo_id: REPO_ZERO,
        large_bcs_id: bonsai::THREES_CSID,
        small_repo_id: REPO_ONE,
        small_bcs_id: None,
        version_name: None,
    };

    let result = mapping
        .insert_equivalent_working_copy(&ctx, null_entry)
        .await
        .expect("Failed to insert working copy");
    assert!(result);

    let should_fail = EquivalentWorkingCopyEntry {
        large_repo_id: REPO_ZERO,
        large_bcs_id: bonsai::THREES_CSID,
        small_repo_id: REPO_ONE,
        small_bcs_id: Some(bonsai::TWOS_CSID),
        version_name: None,
    };
    assert!(
        mapping
            .insert_equivalent_working_copy(&ctx, should_fail)
            .await
            .is_err()
    );
}

#[mononoke::fbinit_test]
async fn test_version_for_large_repo_commit_sql(fb: FacebookInit) -> Result<(), Error> {
    test_version_for_large_repo_commit_impl(fb, sql_synced_commit_mapping()).await
}

#[mononoke::fbinit_test]
async fn test_version_for_large_repo_commit_caching(fb: FacebookInit) -> Result<(), Error> {
    test_version_for_large_repo_commit_impl(fb, caching_synced_commit_mapping()).await
}

async fn test_version_for_large_repo_commit_impl(
    fb: FacebookInit,
    mapping: impl SyncedCommitMapping,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    // Check that at first we don't have a version for a given large repo commit
    assert!(
        mapping
            .get_large_repo_commit_version(&ctx, REPO_ZERO, bonsai::ONES_CSID)
            .await?
            .is_none()
    );

    // Insert working copy equivalence, version for the large commit
    let version_name = CommitSyncConfigVersion("TEST_VERSION_NAME".to_string());
    let entry = EquivalentWorkingCopyEntry {
        large_repo_id: REPO_ZERO,
        large_bcs_id: bonsai::ONES_CSID,
        small_repo_id: REPO_ONE,
        small_bcs_id: Some(bonsai::TWOS_CSID),
        version_name: Some(version_name.clone()),
    };
    let result = mapping
        .insert_equivalent_working_copy(&ctx, entry.clone())
        .await?;
    assert!(result);

    assert_eq!(
        mapping
            .get_large_repo_commit_version(&ctx, REPO_ZERO, bonsai::ONES_CSID)
            .await?,
        Some(version_name.clone())
    );

    // Now try bulk insertion

    let new_version_name = CommitSyncConfigVersion("NEW_TEST_VERSION_NAME".to_string());
    let ctx = CoreContext::test_mock(fb);
    let entry_1 = SyncedCommitMappingEntry::new(
        REPO_ZERO,
        bonsai::THREES_CSID,
        REPO_ONE,
        bonsai::THREES_CSID,
        new_version_name.clone(),
        SyncedCommitSourceRepo::Large,
    );
    let entry_2 = SyncedCommitMappingEntry::new(
        REPO_ZERO,
        bonsai::FOURS_CSID,
        REPO_ONE,
        bonsai::FOURS_CSID,
        new_version_name.clone(),
        SyncedCommitSourceRepo::Large,
    );

    mapping.add_bulk(&ctx, vec![entry_1, entry_2]).await?;

    assert_eq!(
        mapping
            .get_large_repo_commit_version(&ctx, REPO_ZERO, bonsai::THREES_CSID)
            .await?,
        Some(new_version_name.clone())
    );
    assert_eq!(
        mapping
            .get_large_repo_commit_version(&ctx, REPO_ZERO, bonsai::FOURS_CSID)
            .await?,
        Some(new_version_name)
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_overwrite_sql(fb: FacebookInit) -> Result<(), Error> {
    test_overwrite_impl(fb, sql_synced_commit_mapping()).await
}

#[mononoke::fbinit_test]
async fn test_overwrite_caching(fb: FacebookInit) -> Result<(), Error> {
    test_overwrite_impl(fb, caching_synced_commit_mapping()).await
}

async fn test_overwrite_impl(
    fb: FacebookInit,
    mapping: impl SyncedCommitMapping,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);

    // Insert working copy equivalence, version for the large commit
    let version_name = CommitSyncConfigVersion("TEST_VERSION_NAME".to_string());
    let entry = EquivalentWorkingCopyEntry {
        large_repo_id: REPO_ZERO,
        large_bcs_id: bonsai::ONES_CSID,
        small_repo_id: REPO_ONE,
        small_bcs_id: Some(bonsai::TWOS_CSID),
        version_name: Some(version_name.clone()),
    };
    let result = mapping
        .insert_equivalent_working_copy(&ctx, entry.clone())
        .await?;
    assert!(result);

    let new_version_name = CommitSyncConfigVersion("NEW_TEST_VERSION_NAME".to_string());
    let entry = EquivalentWorkingCopyEntry {
        large_repo_id: REPO_ZERO,
        large_bcs_id: bonsai::ONES_CSID,
        small_repo_id: REPO_ONE,
        small_bcs_id: Some(bonsai::TWOS_CSID),
        version_name: Some(new_version_name.clone()),
    };
    let result = mapping
        .overwrite_equivalent_working_copy(&ctx, entry.clone())
        .await?;
    assert!(result);

    let res = mapping
        .get_equivalent_working_copy(&ctx, REPO_ZERO, bonsai::ONES_CSID, REPO_ONE)
        .await
        .expect("get equivalent wc failed, should succeed");

    assert_eq!(
        res,
        Some(WorkingCopyEquivalence::WorkingCopy(
            bonsai::TWOS_CSID,
            new_version_name.clone(),
        ))
    );

    Ok(())
}
