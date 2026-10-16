/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Batch derivation tests for `MappedGitCommitId`.

use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMappingRef;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
use fbinit::FacebookInit;
use git_types::MappedGitCommitId;
use git_types::fetch_non_blob_git_object;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::hash::GitSha1;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use tests_utils::BasicTestRepo;
use tests_utils::CreateCommitContext;

/// A root with its git id derived, as a batch's ancestors must be.
async fn derived_root(ctx: &CoreContext, repo: &BasicTestRepo) -> Result<ChangesetId> {
    let root = CreateCommitContext::new_root(ctx, repo)
        .add_file("base", "base")
        .commit()
        .await?;
    repo.repo_derived_data()
        .manager()
        .derive_locally::<MappedGitCommitId>(ctx, root, None)
        .await?;
    Ok(root)
}

/// `count` children of `parent`, each changing a different single file.
async fn siblings(
    ctx: &CoreContext,
    repo: &BasicTestRepo,
    parent: ChangesetId,
    count: usize,
) -> Result<Vec<ChangesetId>> {
    let mut csids = Vec::with_capacity(count);
    for i in 0..count {
        let csid = CreateCommitContext::new(ctx, repo, vec![parent])
            .add_file(format!("sibling-{i}").as_str(), format!("content {i}"))
            .commit()
            .await?;
        csids.push(csid);
    }
    Ok(csids)
}

/// A parent-to-child chain of `len` commits below `parent`.
async fn chain(
    ctx: &CoreContext,
    repo: &BasicTestRepo,
    parent: ChangesetId,
    len: usize,
) -> Result<Vec<ChangesetId>> {
    let mut csids = Vec::with_capacity(len);
    let mut parent = parent;
    for i in 0..len {
        parent = CreateCommitContext::new(ctx, repo, vec![parent])
            .add_file(format!("link-{i}").as_str(), format!("content {i}"))
            .commit()
            .await?;
        csids.push(parent);
    }
    Ok(csids)
}

/// The oids per-commit `derive` produces for `csids`, from a fresh repo
/// holding the same commits (test commits are content-addressed, so the
/// ids line up).
async fn per_commit_oids(
    ctx: &CoreContext,
    repo: &BasicTestRepo,
    csids: &[ChangesetId],
) -> Result<Vec<GitSha1>> {
    let mut oids = Vec::with_capacity(csids.len());
    for csid in csids {
        let mapped = repo
            .repo_derived_data()
            .derive::<MappedGitCommitId>(ctx, *csid, DerivationPriority::LOW)
            .await?;
        oids.push(*mapped.oid());
    }
    Ok(oids)
}

/// Every commit in the batch has the per-commit oid in the mapping and its
/// git commit object in the blobstore.
async fn assert_batch_matches(
    ctx: &CoreContext,
    repo: &BasicTestRepo,
    csids: &[ChangesetId],
    expected: &[GitSha1],
) -> Result<()> {
    for (csid, expected) in csids.iter().zip(expected) {
        let mapped = repo
            .bonsai_git_mapping()
            .get_git_sha1_from_bonsai(ctx, *csid)
            .await?;
        assert_eq!(
            mapped,
            Some(*expected),
            "{csid} maps to the oid per-commit derivation produces"
        );
        fetch_non_blob_git_object(ctx, repo.repo_blobstore(), &expected.to_object_id()?)
            .await?
            .with_parsed_as_commit(|_| ())
            .expect("the derived object is a stored git commit");
    }
    Ok(())
}

/// Independent siblings in one batch take the concurrent path and end up
/// identical to per-commit derivation.
#[mononoke::fbinit_test]
async fn test_derive_batch_of_independent_siblings(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BasicTestRepo = test_repo_factory::build_empty(fb).await?;
    let reference: BasicTestRepo = test_repo_factory::build_empty(fb).await?;

    let root = derived_root(&ctx, &repo).await?;
    let reference_root = derived_root(&ctx, &reference).await?;
    assert_eq!(root, reference_root, "both repos hold the same commits");
    let csids = siblings(&ctx, &repo, root, 8).await?;
    let reference_csids = siblings(&ctx, &reference, root, 8).await?;
    assert_eq!(csids, reference_csids);

    repo.repo_derived_data()
        .manager()
        .derive_exactly_batch::<MappedGitCommitId>(&ctx, csids.clone(), None)
        .await?;

    let expected = per_commit_oids(&ctx, &reference, &csids).await?;
    assert_batch_matches(&ctx, &repo, &csids, &expected).await?;
    Ok(())
}

/// A chain inside one batch has an in-batch parent, so it takes the
/// sequential path and still derives each link on its predecessor.
#[mononoke::fbinit_test]
async fn test_derive_batch_of_chained_commits(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BasicTestRepo = test_repo_factory::build_empty(fb).await?;
    let reference: BasicTestRepo = test_repo_factory::build_empty(fb).await?;

    let root = derived_root(&ctx, &repo).await?;
    derived_root(&ctx, &reference).await?;
    let csids = chain(&ctx, &repo, root, 3).await?;
    let reference_csids = chain(&ctx, &reference, root, 3).await?;
    assert_eq!(csids, reference_csids);

    repo.repo_derived_data()
        .manager()
        .derive_exactly_batch::<MappedGitCommitId>(&ctx, csids.clone(), None)
        .await?;

    let expected = per_commit_oids(&ctx, &reference, &csids).await?;
    assert_batch_matches(&ctx, &repo, &csids, &expected).await?;

    let parents =
        fetch_non_blob_git_object(&ctx, repo.repo_blobstore(), &expected[2].to_object_id()?)
            .await?
            .with_parsed_as_commit(|commit| commit.parents().collect::<Vec<_>>())
            .expect("the derived object is a commit");
    assert_eq!(
        parents,
        vec![expected[1].to_object_id()?],
        "the last link's git parent is the previous link"
    );
    Ok(())
}
