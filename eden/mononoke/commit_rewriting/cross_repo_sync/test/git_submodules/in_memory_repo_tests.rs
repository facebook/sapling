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
use changesets::ChangesetEntry;
use changesets::ChangesetsRef;
use changesets_creation::save_changesets;
use context::CoreContext;
use cross_repo_sync::InMemoryRepo;
use fbinit::FacebookInit;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::DateTime;
use mononoke_types::RepositoryId;
use repo_blobstore::RepoBlobstoreRef;
use sorted_vector_map::SortedVectorMap;

use crate::git_submodules::git_submodules_test_utils::*;

#[fbinit::test]
async fn test_original_blobstore_and_changesets_are_the_same_after_validation(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (fallback_repo, _fallback_repo_cs_map) = build_repo_c(fb).await?;

    let (orig_repo, _orig_repo_cs_map) = build_repo_b(fb).await?;

    let fallback_repos = vec![Arc::new(fallback_repo.clone())];
    // Create an InMemoryRepo from the original repo
    let in_memory_repo = InMemoryRepo::from_repo(&orig_repo, fallback_repos)?;

    // Create a new bonsai changeset using the in_memory_repo
    let new_bonsai_mut = BonsaiChangesetMut {
        parents: vec![],
        message: "Create directory in repo B".into(),
        file_changes: SortedVectorMap::new(),
        author_date: DateTime::from_timestamp(0, 0).unwrap(),
        ..Default::default()
    };

    let bonsai = new_bonsai_mut.freeze().unwrap();
    let cs_id = bonsai.get_changeset_id();
    // Save that changeset
    save_changesets(&ctx, &in_memory_repo, vec![bonsai])
        .await
        .context("Failed to save changesets")?;

    // 1. Changeset can be loaded from in_memory_repo
    let from_in_memory = cs_id.load(&ctx, &in_memory_repo.repo_blobstore()).await?;
    println!("from_in_memory: {0:#?}", from_in_memory);

    let from_orig_repo = cs_id.load(&ctx, &orig_repo.repo_blobstore()).await;

    // 2. Changeset can't be loaded from original repo
    println!(
        "Loading changeset from original repo: {0:#?}",
        from_orig_repo
    );
    assert!(from_orig_repo.is_err_and(|e| {
        // Fails to find changeset blob
        e.to_string().contains("Blob is missing: changeset.blake2")
    }));

    // 3. Changeset entry can be found using the in_memory_repo
    let in_memory_changesets = in_memory_repo.changesets();
    let in_mememory_changeset = in_memory_changesets.get(&ctx, cs_id).await?;

    assert_eq!(
        in_mememory_changeset,
        Some(ChangesetEntry {
            repo_id: RepositoryId::new(2),
            cs_id,
            parents: vec![],
            gen: 1,
        })
    );

    // 4. Changeset entry can't be found using the original repo
    let orig_changesets = orig_repo.changesets();
    let orig_changeset = orig_changesets.get(&ctx, cs_id).await?;

    println!("orig_changeset: {0:#?}", &orig_changeset);

    assert!(
        orig_changeset.is_none(),
        "Changeset was found in original repo when it shouldn't be"
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
    let _ = bonsai.get_changeset_id();

    println!("fallback_changeset_bonsai: {0:#?}", &bonsai);

    // Save that changeset
    save_changesets(&ctx, &fallback_repo, vec![bonsai])
        .await
        .context("Failed to save changesets")?;

    // 5. Fallback repo changeset can be loaded from in_memory repo
    let fallback_changeset_in_memory = cs_id.load(&ctx, &in_memory_repo.repo_blobstore()).await?;
    println!(
        "fallback_changeset_in_memory: {0:#?}",
        fallback_changeset_in_memory
    );

    // 6. Fallback repo bonsai can't be loaded from original repo
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
