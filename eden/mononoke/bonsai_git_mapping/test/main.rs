/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Error;
use bonsai_git_mapping::{
    BonsaiGitMapping, BonsaiGitMappingEntry, BonsaisOrGitShas, SqlBonsaiGitMapping,
};
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::hash::*;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql_ext::SqlConstructors;

#[fbinit::test]
async fn test_add_and_get() -> Result<(), Error> {
    let mapping = SqlBonsaiGitMapping::with_sqlite_in_memory()?;

    let entry = BonsaiGitMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::ONES_CSID,
        git_sha1: ONES_GIT_SHA1,
    };

    mapping.bulk_add(&vec![entry.clone()]).await?;

    let result = mapping
        .get(REPO_ZERO, BonsaisOrGitShas::Bonsai(vec![bonsai::ONES_CSID]))
        .await?;
    assert_eq!(result, vec![entry.clone()]);
    let result = mapping
        .get_git_sha1_from_bonsai(REPO_ZERO, bonsai::ONES_CSID)
        .await?;
    assert_eq!(result, Some(ONES_GIT_SHA1));
    let result = mapping
        .get_bonsai_from_git_sha1(REPO_ZERO, ONES_GIT_SHA1)
        .await?;
    assert_eq!(result, Some(bonsai::ONES_CSID));

    Ok(())
}

#[fbinit::test]
async fn test_bulk_add() -> Result<(), Error> {
    let mapping = SqlBonsaiGitMapping::with_sqlite_in_memory()?;

    let entry1 = BonsaiGitMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::ONES_CSID,
        git_sha1: ONES_GIT_SHA1,
    };
    let entry2 = BonsaiGitMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::TWOS_CSID,
        git_sha1: TWOS_GIT_SHA1,
    };
    let entry3 = BonsaiGitMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::THREES_CSID,
        git_sha1: THREES_GIT_SHA1,
    };

    mapping
        .bulk_add(&vec![entry1.clone(), entry2.clone(), entry3.clone()])
        .await?;

    Ok(())
}

#[fbinit::test]
async fn test_missing() -> Result<(), Error> {
    let mapping = SqlBonsaiGitMapping::with_sqlite_in_memory()?;

    let result = mapping
        .get(REPO_ZERO, BonsaisOrGitShas::Bonsai(vec![bonsai::ONES_CSID]))
        .await?;

    assert_eq!(result, vec![]);

    Ok(())
}
