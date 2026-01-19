/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Error;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use mononoke_types::Timestamp;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::REPO_ZERO;
use repo_metadata_checkpoint::RepoMetadataCheckpoint;
use repo_metadata_checkpoint::RepoMetadataCheckpointEntry;
use repo_metadata_checkpoint::RepoMetadataFullRunInfo;
use repo_metadata_checkpoint::SqlRepoMetadataCheckpointBuilder;
use repo_metadata_checkpoint::SqlRepoMetadataFullRunInfoBuilder;
use sql_construct::SqlConstruct;

#[mononoke::fbinit_test]
async fn test_add_and_get(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let metadata_info = SqlRepoMetadataCheckpointBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, ctx.sql_query_telemetry());
    let bookmark_name = "JustABookmark";
    let timestamp = Timestamp::now();
    let entry = RepoMetadataCheckpointEntry {
        changeset_id: bonsai::ONES_CSID,
        bookmark_name: bookmark_name.to_string(),
        last_updated_timestamp: timestamp,
    };
    metadata_info
        .add_or_update_entries(vec![entry.clone()])
        .await?;
    let result = metadata_info.get_entry(entry.bookmark_name.clone()).await?;

    assert_eq!(
        result.as_ref().map(|entry| (
            entry.bookmark_name.to_string(),
            entry.changeset_id,
            entry.last_updated_timestamp
        )),
        Some((bookmark_name.to_string(), bonsai::ONES_CSID, timestamp))
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_update_and_get(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let metadata_info = SqlRepoMetadataCheckpointBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, ctx.sql_query_telemetry());
    let bookmark_name = "JustABookmark";
    let timestamp = Timestamp::now();
    let entry = RepoMetadataCheckpointEntry {
        changeset_id: bonsai::ONES_CSID,
        bookmark_name: bookmark_name.to_string(),
        last_updated_timestamp: timestamp,
    };

    metadata_info
        .add_or_update_entries(vec![entry.clone()])
        .await?;
    let result = metadata_info.get_entry(entry.bookmark_name.clone()).await?;
    assert_eq!(result, Some(entry.clone()));

    // Update the changeset id and try storing the same entry
    let updated_timestamp = Timestamp::now();
    let new_entry = RepoMetadataCheckpointEntry {
        changeset_id: bonsai::TWOS_CSID,
        bookmark_name: bookmark_name.to_string(),
        last_updated_timestamp: updated_timestamp,
    };

    metadata_info
        .add_or_update_entries(vec![new_entry.clone()])
        .await?;

    let result = metadata_info
        .get_entry(new_entry.bookmark_name.clone())
        .await?;
    assert_eq!(result, Some(new_entry.clone()));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_without_add(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let metadata_info = SqlRepoMetadataCheckpointBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, ctx.sql_query_telemetry());
    let result = metadata_info.get_entry("master".to_string()).await?;
    assert_eq!(result, None);

    let result = metadata_info.get_all_entries().await?;
    assert_eq!(result.len(), 0);

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_multiple(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let metadata_info = SqlRepoMetadataCheckpointBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, ctx.sql_query_telemetry());
    let entry = RepoMetadataCheckpointEntry {
        changeset_id: bonsai::ONES_CSID,
        bookmark_name: "master".to_string(),
        last_updated_timestamp: Timestamp::now(),
    };
    let another_entry = RepoMetadataCheckpointEntry {
        changeset_id: bonsai::TWOS_CSID,
        bookmark_name: "release".to_string(),
        last_updated_timestamp: Timestamp::now(),
    };

    metadata_info
        .add_or_update_entries(vec![entry.clone(), another_entry.clone()])
        .await?;

    let result = metadata_info
        .get_all_entries()
        .await?
        .into_iter()
        .map(|entry| entry.bookmark_name);
    assert_eq!(
        HashSet::from_iter(result),
        HashSet::from(["master".to_string(), "release".to_string()])
    );
    Ok(())
}

// Tests for RepoMetadataFullRunInfo

#[mononoke::fbinit_test]
async fn test_full_run_info_get_without_set(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let full_run_info = SqlRepoMetadataFullRunInfoBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, ctx.sql_query_telemetry());

    // Should return None when no timestamp has been set
    let result = full_run_info.get_last_full_run_timestamp().await?;
    assert_eq!(result, None);

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_full_run_info_set_and_get(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let full_run_info = SqlRepoMetadataFullRunInfoBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, ctx.sql_query_telemetry());

    let timestamp = Timestamp::from_timestamp_secs(1000000);

    // Set the timestamp
    full_run_info.set_last_full_run_timestamp(timestamp).await?;

    // Should return the timestamp we set
    let result = full_run_info.get_last_full_run_timestamp().await?;
    assert_eq!(result, Some(timestamp));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_full_run_info_update(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let full_run_info = SqlRepoMetadataFullRunInfoBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, ctx.sql_query_telemetry());

    let timestamp1 = Timestamp::from_timestamp_secs(1000000);
    let timestamp2 = Timestamp::from_timestamp_secs(2000000);

    // Set initial timestamp
    full_run_info
        .set_last_full_run_timestamp(timestamp1)
        .await?;
    assert_eq!(
        full_run_info.get_last_full_run_timestamp().await?,
        Some(timestamp1)
    );

    // Update to new timestamp
    full_run_info
        .set_last_full_run_timestamp(timestamp2)
        .await?;
    assert_eq!(
        full_run_info.get_last_full_run_timestamp().await?,
        Some(timestamp2)
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_full_run_info_repo_id(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let full_run_info = SqlRepoMetadataFullRunInfoBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, ctx.sql_query_telemetry());

    // Verify repo_id is correctly stored
    assert_eq!(full_run_info.repo_id(), REPO_ZERO);

    Ok(())
}
