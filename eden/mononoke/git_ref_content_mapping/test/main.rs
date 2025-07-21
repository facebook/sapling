/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::str::FromStr;

use anyhow::Error;
use context::CoreContext;
use fbinit::FacebookInit;
use git_ref_content_mapping::GitRefContentMapping;
use git_ref_content_mapping::GitRefContentMappingEntry;
use git_ref_content_mapping::SqlGitRefContentMappingBuilder;
use mononoke_macros::mononoke;
use mononoke_types::hash::GitSha1;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql_construct::SqlConstruct;

const ZERO_GIT_HASH: &str = "0000000000000000000000000000000000000000";
const ONE_GIT_HASH: &str = "1111111111111111111111111111111111111111";

#[mononoke::fbinit_test]
async fn test_add_and_get(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlGitRefContentMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let ref_name = "refs/tags/tag_to_tree";
    let entry = GitRefContentMappingEntry {
        ref_name: ref_name.to_string(),
        git_hash: GitSha1::from_str(ZERO_GIT_HASH)?,
        is_tree: true,
    };

    mapping
        .add_or_update_mappings(&ctx, vec![entry.clone()])
        .await?;

    let result = mapping
        .get_entry_by_ref_name(&ctx, entry.ref_name.clone())
        .await?;
    assert_eq!(
        result.as_ref().map(|entry| entry.git_hash),
        Some(GitSha1::from_str(ZERO_GIT_HASH)?)
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_update_and_get(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlGitRefContentMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let ref_name = "refs/tags/tag_to_tree";
    let entry = GitRefContentMappingEntry {
        ref_name: ref_name.to_string(),
        git_hash: GitSha1::from_str(ZERO_GIT_HASH)?,
        is_tree: true,
    };

    mapping
        .add_or_update_mappings(&ctx, vec![entry.clone()])
        .await?;

    let result = mapping
        .get_entry_by_ref_name(&ctx, entry.ref_name.clone())
        .await?;
    assert_eq!(result, Some(entry.clone()));

    // Update the git hash and try storing the same entry
    let new_entry = GitRefContentMappingEntry {
        ref_name: ref_name.to_string(),
        git_hash: GitSha1::from_str(ONE_GIT_HASH)?,
        is_tree: true,
    };
    mapping
        .add_or_update_mappings(&ctx, vec![new_entry.clone()])
        .await?;
    let result = mapping
        .get_entry_by_ref_name(&ctx, new_entry.ref_name.clone())
        .await?;
    assert_eq!(result, Some(new_entry.clone()));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_without_add(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlGitRefContentMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let result = mapping
        .get_entry_by_ref_name(&ctx, "refs/tags/tag_to_tree".to_string())
        .await?;
    assert_eq!(result, None);

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_all_mappings(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlGitRefContentMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let entry = GitRefContentMappingEntry {
        ref_name: "refs/tags/tag_to_tree".to_string(),
        git_hash: GitSha1::from_str(ZERO_GIT_HASH)?,
        is_tree: true,
    };
    let another_entry = GitRefContentMappingEntry {
        ref_name: "refs/tags/tag_to_blob".to_string(),
        git_hash: GitSha1::from_str(ONE_GIT_HASH)?,
        is_tree: false,
    };
    mapping
        .add_or_update_mappings(&ctx, vec![entry, another_entry])
        .await?;

    let result = mapping
        .get_all_entries(&ctx)
        .await?
        .into_iter()
        .map(|entry| entry.ref_name);
    assert_eq!(
        HashSet::from_iter(result),
        HashSet::from([
            "refs/tags/tag_to_tree".to_string(),
            "refs/tags/tag_to_blob".to_string()
        ])
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_delete_mappings_by_name(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlGitRefContentMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let entry = GitRefContentMappingEntry {
        ref_name: "refs/tags/tag_to_tree".to_string(),
        git_hash: GitSha1::from_str(ZERO_GIT_HASH)?,
        is_tree: true,
    };
    let another_entry = GitRefContentMappingEntry {
        ref_name: "refs/tags/tag_to_blob".to_string(),
        git_hash: GitSha1::from_str(ONE_GIT_HASH)?,
        is_tree: false,
    };
    mapping
        .add_or_update_mappings(&ctx, vec![entry, another_entry])
        .await?;

    let result = mapping
        .get_all_entries(&ctx)
        .await?
        .into_iter()
        .map(|entry| entry.ref_name);
    assert_eq!(
        HashSet::from_iter(result),
        HashSet::from([
            "refs/tags/tag_to_tree".to_string(),
            "refs/tags/tag_to_blob".to_string()
        ])
    );

    mapping
        .delete_mappings_by_name(
            &ctx,
            vec![
                "refs/tags/tag_to_tree".to_string(),
                "refs/tags/tag_to_blob".to_string(),
            ],
        )
        .await?;

    assert!(mapping.get_all_entries(&ctx).await?.is_empty());
    Ok(())
}
