/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Error;
use bonsai_tag_mapping::BonsaiTagMapping;
use bonsai_tag_mapping::BonsaiTagMappingEntry;
use bonsai_tag_mapping::SqlBonsaiTagMappingBuilder;
use fbinit::FacebookInit;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql_construct::SqlConstruct;

#[fbinit::test]
async fn test_add_and_get(_: FacebookInit) -> Result<(), Error> {
    let mapping = SqlBonsaiTagMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let tag_name = "JustATag";
    let entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::ONES_CSID,
        tag_name: tag_name.to_string(),
    };

    mapping.add_mappings(vec![entry.clone()]).await?;

    let result = mapping
        .get_changeset_by_tag_name(entry.tag_name.clone())
        .await?;
    assert_eq!(result, Some(bonsai::ONES_CSID));

    let result = mapping
        .get_tag_names_by_changeset(bonsai::ONES_CSID)
        .await?;
    assert_eq!(result, Some(vec![tag_name.to_string()]));
    Ok(())
}

#[fbinit::test]
async fn test_get_without_add(_: FacebookInit) -> Result<(), Error> {
    let mapping = SqlBonsaiTagMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let result = mapping
        .get_changeset_by_tag_name("JustATag".to_string())
        .await?;
    assert_eq!(result, None);

    let result = mapping
        .get_tag_names_by_changeset(bonsai::ONES_CSID)
        .await?;
    assert_eq!(result, None);
    Ok(())
}

#[fbinit::test]
async fn test_get_multiple_tags(_: FacebookInit) -> Result<(), Error> {
    let mapping = SqlBonsaiTagMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::ONES_CSID,
        tag_name: "JustATag".to_string(),
    };
    let another_entry = BonsaiTagMappingEntry {
        changeset_id: bonsai::ONES_CSID,
        tag_name: "AnotherTag".to_string(),
    };
    mapping.add_mappings(vec![entry, another_entry]).await?;

    let result = mapping
        .get_tag_names_by_changeset(bonsai::ONES_CSID)
        .await?
        .expect("None tags returned for the input changeset");
    assert_eq!(
        HashSet::from_iter(result),
        HashSet::from(["JustATag".to_string(), "AnotherTag".to_string()])
    );
    Ok(())
}
