/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests for the InMemoryRepo used in submodule expansion validation

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use blobstore::Loadable;
use changesets_creation::save_changesets;
use context::CoreContext;
use cross_repo_sync::InMemoryRepo;
use fbinit::FacebookInit;
use fsnodes::RootFsnodeId;
use mononoke_macros::mononoke;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::DateTime;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use sorted_vector_map::SortedVectorMap;

use crate::git_submodules::git_submodules_test_utils::*;

#[mononoke::fbinit_test]
async fn test_original_blobstore_and_changesets_are_the_same_after_validation(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (fallback_repo, _fallback_repo_cs_map) = build_repo_c(fb).await?;

    let (orig_repo, orig_repo_cs_map) = build_repo_b(fb).await?;

    let fallback_repos = vec![Arc::new(fallback_repo.clone())];
    // Create an InMemoryRepo from the original repo
    let in_memory_repo = InMemoryRepo::from_repo(&orig_repo, fallback_repos)?;

    let orig_repo_commit = *orig_repo_cs_map.get("B_B").unwrap();

    // Derive Fsnodes for a commit in the InMemoryRepo
    in_memory_repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(&ctx, orig_repo_commit)
        .await?;

    // Check that Fsnodes are not derived for that commit in the original repo
    assert!(
        orig_repo
            .repo_derived_data()
            .fetch_derived::<RootFsnodeId>(&ctx, orig_repo_commit)
            .await?
            .is_none()
    );

    // ------------------ Test fallback repo ------------------

    // Create a new bonsai changeset in fallback repo
    let new_bonsai_mut = BonsaiChangesetMut {
        parents: vec![],
        message: "Create directory in repo C".into(),
        file_changes: SortedVectorMap::new(),
        author_date: DateTime::from_timestamp(0, 0).unwrap(),
        ..Default::default()
    };

    let bonsai = new_bonsai_mut.freeze().unwrap();
    let cs_id = bonsai.get_changeset_id();

    println!("fallback_changeset_bonsai: {0:#?}", &bonsai);

    // Save that changeset
    save_changesets(&ctx, &fallback_repo, vec![bonsai])
        .await
        .context("Failed to save changesets")?;

    // Fallback repo changeset can be loaded from in_memory repo
    let fallback_changeset_in_memory = cs_id.load(&ctx, &in_memory_repo.repo_blobstore()).await?;
    println!(
        "fallback_changeset_in_memory: {0:#?}",
        fallback_changeset_in_memory
    );

    // Fallback repo bonsai can't be loaded from original repo
    let fallback_changeset_orig_repo = cs_id.load(&ctx, &orig_repo.repo_blobstore()).await;
    println!(
        "Loading Fallback repo changeset from original repo: {0:#?}",
        fallback_changeset_orig_repo
    );
    assert!(fallback_changeset_orig_repo.is_err_and(|e| {
        // Fails to find changeset blob
        e.to_string().contains("Blob is missing: changeset.blake2")
    }));

    Ok(())
}
