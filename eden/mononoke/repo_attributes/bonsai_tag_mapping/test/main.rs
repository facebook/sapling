/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use bonsai_tag_mapping::BonsaiTagMapping;
use bonsai_tag_mapping::BonsaiTagMappingEntry;
use bonsai_tag_mapping::CachedBonsaiTagMapping;
use bonsai_tag_mapping::Freshness;
use bonsai_tag_mapping::SqlBonsaiTagMappingBuilder;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::FutureExt;
use justknobs::test_helpers::JustKnobsInMemory;
use justknobs::test_helpers::KnobVal;
use justknobs::test_helpers::with_just_knobs_async;
use maplit::hashmap;
use mononoke_macros::mononoke;
use mononoke_types::hash::GitSha1;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::REPO_ZERO;
use rendezvous::RendezVousOptions;
use repo_update_logger::PlainBookmarkInfo;
use sql_construct::SqlConstruct;
use tokio::sync::broadcast;
use tokio::time::sleep;

const ZERO_GIT_HASH: &str = "0000000000000000000000000000000000000000";
const ONE_GIT_HASH: &str = "1111111111111111111111111111111111111111";

#[mononoke::fbinit_test]
async fn test_add_and_get(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiTagMappingBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, RendezVousOptions::for_test());
    let tag_name = "JustATag";
    let entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::ONES_CSID,
        tag_name: tag_name.to_string(),
        tag_hash: GitSha1::from_str(ZERO_GIT_HASH)?,
        target_is_tag: false,
    };

    mapping
        .add_or_update_mappings(&ctx, vec![entry.clone()])
        .await?;

    let result = mapping
        .get_entry_by_tag_name(&ctx, entry.tag_name.clone(), Freshness::MaybeStale)
        .await?;
    assert_eq!(
        result.as_ref().map(|entry| entry.changeset_id),
        Some(bonsai::ONES_CSID)
    );
    assert_eq!(
        result.as_ref().map(|entry| entry.target_is_tag),
        Some(false)
    );

    let result = mapping
        .get_entries_by_changeset(&ctx, bonsai::ONES_CSID)
        .await?
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| entry.tag_name)
                .collect::<Vec<_>>()
        });
    assert_eq!(result, Some(vec![tag_name.to_string()]));
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_update_and_get(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiTagMappingBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, RendezVousOptions::for_test());
    let tag_name = "JustATag";
    let entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::ONES_CSID,
        tag_name: tag_name.to_string(),
        tag_hash: GitSha1::from_str(ZERO_GIT_HASH)?,
        target_is_tag: false,
    };

    mapping
        .add_or_update_mappings(&ctx, vec![entry.clone()])
        .await?;

    let result = mapping
        .get_entry_by_tag_name(&ctx, entry.tag_name.clone(), Freshness::Latest)
        .await?;
    assert_eq!(result, Some(entry.clone()));

    let result = mapping
        .get_entries_by_changeset(&ctx, bonsai::ONES_CSID)
        .await?;
    assert_eq!(result, Some(vec![entry]));

    // Update the tag hash and try storing the same entry
    let new_entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::ONES_CSID,
        tag_name: tag_name.to_string(),
        tag_hash: GitSha1::from_str(ONE_GIT_HASH)?,
        target_is_tag: true,
    };
    mapping
        .add_or_update_mappings(&ctx, vec![new_entry.clone()])
        .await?;
    let result = mapping
        .get_entry_by_tag_name(&ctx, new_entry.tag_name.clone(), Freshness::Latest)
        .await?;
    assert_eq!(result, Some(new_entry.clone()));

    let result = mapping
        .get_entries_by_changeset(&ctx, bonsai::ONES_CSID)
        .await?;
    assert_eq!(result, Some(vec![new_entry]));
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_without_add(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiTagMappingBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, RendezVousOptions::for_test());
    let result = mapping
        .get_entry_by_tag_name(&ctx, "JustATag".to_string(), Freshness::MaybeStale)
        .await?;
    assert_eq!(result, None);

    let result = mapping
        .get_entries_by_changeset(&ctx, bonsai::ONES_CSID)
        .await?;
    assert_eq!(result, None);
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_multiple_tags(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiTagMappingBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, RendezVousOptions::for_test());
    let entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::ONES_CSID,
        tag_name: "JustATag".to_string(),
        tag_hash: GitSha1::from_str(ZERO_GIT_HASH)?,
        target_is_tag: false,
    };
    let another_entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::ONES_CSID,
        tag_name: "AnotherTag".to_string(),
        tag_hash: GitSha1::from_str(ONE_GIT_HASH)?,
        target_is_tag: true,
    };
    mapping
        .add_or_update_mappings(&ctx, vec![entry, another_entry])
        .await?;

    let result = mapping
        .get_entries_by_changeset(&ctx, bonsai::ONES_CSID)
        .await?
        .expect("None tags returned for the input changeset")
        .into_iter()
        .map(|entry| entry.tag_name);
    assert_eq!(
        HashSet::from_iter(result),
        HashSet::from(["JustATag".to_string(), "AnotherTag".to_string()])
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_tags_by_multiple_changesets(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiTagMappingBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, RendezVousOptions::for_test());
    let entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::ONES_CSID,
        tag_name: "JustATag".to_string(),
        tag_hash: GitSha1::from_str(ZERO_GIT_HASH)?,
        target_is_tag: false,
    };
    let another_entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::TWOS_CSID,
        tag_name: "AnotherTag".to_string(),
        tag_hash: GitSha1::from_str(ONE_GIT_HASH)?,
        target_is_tag: true,
    };
    mapping
        .add_or_update_mappings(&ctx, vec![entry, another_entry])
        .await?;

    let result = mapping
        .get_entries_by_changesets(&ctx, vec![bonsai::ONES_CSID, bonsai::TWOS_CSID])
        .await?
        .into_iter()
        .map(|entry| entry.tag_name);
    assert_eq!(
        HashSet::from_iter(result),
        HashSet::from(["JustATag".to_string(), "AnotherTag".to_string()])
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_all_tags(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiTagMappingBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, RendezVousOptions::for_test());
    let entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::ONES_CSID,
        tag_name: "JustATag".to_string(),
        tag_hash: GitSha1::from_str(ZERO_GIT_HASH)?,
        target_is_tag: false,
    };
    let another_entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::TWOS_CSID,
        tag_name: "AnotherTag".to_string(),
        tag_hash: GitSha1::from_str(ONE_GIT_HASH)?,
        target_is_tag: true,
    };
    mapping
        .add_or_update_mappings(&ctx, vec![entry, another_entry])
        .await?;

    let result = mapping
        .get_all_entries(&ctx)
        .await?
        .into_iter()
        .map(|entry| entry.tag_name);
    assert_eq!(
        HashSet::from_iter(result),
        HashSet::from(["JustATag".to_string(), "AnotherTag".to_string()])
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_tag_by_tag_hashes(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiTagMappingBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, RendezVousOptions::for_test());
    let entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::ONES_CSID,
        tag_name: "JustATag".to_string(),
        tag_hash: GitSha1::from_str(ZERO_GIT_HASH)?,
        target_is_tag: false,
    };
    let another_entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::TWOS_CSID,
        tag_name: "AnotherTag".to_string(),
        tag_hash: GitSha1::from_str(ONE_GIT_HASH)?,
        target_is_tag: true,
    };
    mapping
        .add_or_update_mappings(&ctx, vec![entry, another_entry])
        .await?;

    let result = mapping
        .get_entries_by_tag_hashes(
            &ctx,
            vec![
                GitSha1::from_str(ZERO_GIT_HASH)?,
                GitSha1::from_str(ONE_GIT_HASH)?,
            ],
        )
        .await?
        .into_iter()
        .map(|entry| entry.tag_name);
    assert_eq!(
        HashSet::from_iter(result),
        HashSet::from(["JustATag".to_string(), "AnotherTag".to_string()])
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_delete_mappings_by_name(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiTagMappingBuilder::with_sqlite_in_memory()?
        .build(REPO_ZERO, RendezVousOptions::for_test());
    let entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::ONES_CSID,
        tag_name: "JustATag".to_string(),
        tag_hash: GitSha1::from_str(ZERO_GIT_HASH)?,
        target_is_tag: false,
    };
    let another_entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::TWOS_CSID,
        tag_name: "AnotherTag".to_string(),
        tag_hash: GitSha1::from_str(ONE_GIT_HASH)?,
        target_is_tag: true,
    };
    mapping
        .add_or_update_mappings(&ctx, vec![entry, another_entry])
        .await?;

    let result = mapping
        .get_all_entries(&ctx)
        .await?
        .into_iter()
        .map(|entry| entry.tag_name);
    assert_eq!(
        HashSet::from_iter(result),
        HashSet::from(["JustATag".to_string(), "AnotherTag".to_string()])
    );

    mapping
        .delete_mappings_by_name(&ctx, vec!["JustATag".to_string(), "AnotherTag".to_string()])
        .await?;

    assert!(mapping.get_all_entries(&ctx,).await?.is_empty());
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_cached_bonsai_tag_mappings(fb: FacebookInit) -> Result<(), Error> {
    with_just_knobs_async(
        JustKnobsInMemory::new(hashmap![
            "scm/mononoke:enable_bonsai_tag_mapping_caching".to_string() => KnobVal::Bool(true),
        ]),
        async {
            let mapping = Arc::new(
                SqlBonsaiTagMappingBuilder::with_sqlite_in_memory()?
                    .build(REPO_ZERO, RendezVousOptions::for_test()),
            );
            let ctx = CoreContext::test_mock(fb);
            let (sender, receiver) = broadcast::channel(10);
            let mapping = CachedBonsaiTagMapping::new(&ctx, mapping, receiver).await?;
            // Add a few mappings
            let entry = BonsaiTagMappingEntry {
                changeset_id: bonsai::ONES_CSID,
                tag_name: "JustATag".to_string(),
                tag_hash: GitSha1::from_str(ZERO_GIT_HASH)?,
                target_is_tag: false,
            };
            let another_entry = BonsaiTagMappingEntry {
                changeset_id: bonsai::TWOS_CSID,
                tag_name: "AnotherTag".to_string(),
                tag_hash: GitSha1::from_str(ONE_GIT_HASH)?,
                target_is_tag: true,
            };
            mapping
                .add_or_update_mappings(&ctx, vec![entry, another_entry])
                .await?;
            let received_by = sender.send(PlainBookmarkInfo::default())?;
            assert_eq!(
                received_by, 1,
                "The number of receivers for tag update does not match the expected count"
            );
            sleep(Duration::from_secs(3)).await;
            // Ensure that the cache got updated with the latest value
            let result = mapping
                .get_all_entries(&ctx)
                .await?
                .into_iter()
                .map(|entry| entry.tag_name);
            assert_eq!(
                HashSet::from_iter(result),
                HashSet::from(["JustATag".to_string(), "AnotherTag".to_string()])
            );
            // Update the tag hash and try storing the same entry. Validate that the cache catches up with that change
            let new_entry = BonsaiTagMappingEntry {
                changeset_id: bonsai::ONES_CSID,
                tag_name: "JustATag".to_string(),
                tag_hash: GitSha1::from_str(ONE_GIT_HASH)?,
                target_is_tag: true,
            };
            mapping
                .add_or_update_mappings(&ctx, vec![new_entry.clone()])
                .await?;
            sender.send(PlainBookmarkInfo::default())?;
            sleep(Duration::from_secs(3)).await;
            let result = mapping
                .get_entry_by_tag_name(&ctx, new_entry.tag_name.clone(), Freshness::MaybeStale) // MaybeStale so that the cache is used
                .await?;
            assert_eq!(result, Some(new_entry.clone()));
            // Remove all entries and validate that the cache catches up with that change
            mapping
                .delete_mappings_by_name(
                    &ctx,
                    vec!["JustATag".to_string(), "AnotherTag".to_string()],
                )
                .await?;
            sender.send(PlainBookmarkInfo::default())?;
            sleep(Duration::from_secs(3)).await;
            assert!(mapping.get_all_entries(&ctx,).await?.is_empty());
            Ok(())
        }
        .boxed(),
    )
    .await
}
