/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::Loadable;
use bonsai_git_mapping::BonsaiGitMappingEntry;
use bonsai_git_mapping::BonsaiGitMappingRef;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::hash::GitSha1;
use multi_repo_land_lib::PREDECESSOR_HOP_MAX_BRANCH_COMMITS;
use multi_repo_land_lib::RecordedPredecessorHop;
use multi_repo_land_lib::describe_commit_for_client;
use multi_repo_land_lib::differential_revision;
use pushrebase::BaselineCandidateSource;
use pushrebase_mutation_mapping::PushrebaseMutationMappingEntry;
use pushrebase_mutation_mapping::add_pushrebase_mapping;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use test_repo_factory::TestRepoFactory;
use tests_utils::CreateCommitContext;

use crate::TestRepo;

async fn give_git_identity(
    ctx: &CoreContext,
    repo: &TestRepo,
    cs_id: ChangesetId,
    byte: u8,
) -> Result<GitSha1> {
    let git_sha1 = GitSha1::from_byte_array([byte; 20]);
    repo.bonsai_git_mapping()
        .bulk_add(ctx, &[BonsaiGitMappingEntry::new(git_sha1, cs_id)])
        .await?;
    Ok(git_sha1)
}

async fn record_rewrite(
    ctx: &CoreContext,
    repo: &TestRepo,
    predecessor: ChangesetId,
    successor: ChangesetId,
) -> Result<()> {
    let txn = repo
        .sql_bookmarks
        .write_connection()
        .start_transaction(ctx.sql_query_telemetry())
        .await?;
    let txn = add_pushrebase_mapping(
        txn,
        &[PushrebaseMutationMappingEntry::new(
            repo.repo_identity().id(),
            predecessor,
            successor,
        )],
    )
    .await?;
    txn.commit().await?;
    Ok(())
}

/// A commit as a client's auto-rebase uploads it: `predecessor` and
/// `predecessor-op` headers.
async fn commit_naming_predecessor(
    ctx: &CoreContext,
    repo: &TestRepo,
    parent: ChangesetId,
    path: &str,
    predecessor: GitSha1,
    op: &str,
    author: Option<&str>,
) -> Result<ChangesetId> {
    let mut plain = CreateCommitContext::new(ctx, repo, vec![parent]).add_file(path, "content");
    if let Some(author) = author {
        plain = plain.set_author(author);
    }
    let plain = plain.commit().await?;
    let mut bcs = plain.load(ctx, repo.repo_blobstore()).await?.into_mut();
    bcs.git_extra_headers = Some(
        [
            (
                b"predecessor".iter().copied().collect(),
                bytes::Bytes::from(predecessor.to_hex().to_string()),
            ),
            (
                b"predecessor-op".iter().copied().collect(),
                bytes::Bytes::from(op.to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let bcs = bcs.freeze()?;
    let cs_id = bcs.get_changeset_id();
    changesets_creation::save_changesets(ctx, repo, vec![bcs]).await?;
    Ok(cs_id)
}

struct Hop {
    fork: ChangesetId,
    v1: ChangesetId,
    v1_git: GitSha1,
    landed: ChangesetId,
    head: ChangesetId,
}

/// V1 on the fork point with a git identity; the branch has a landed form
/// and a head above it.
async fn hop(ctx: &CoreContext, repo: &TestRepo) -> Result<Hop> {
    let fork = CreateCommitContext::new_root(ctx, repo)
        .add_file("file_base", "base")
        .commit()
        .await?;
    let v1 = CreateCommitContext::new(ctx, repo, vec![fork])
        .add_file("file_parent", "p1")
        .commit()
        .await?;
    let v1_git = give_git_identity(ctx, repo, v1, 0x11).await?;
    let landed = CreateCommitContext::new(ctx, repo, vec![fork])
        .add_file("file_parent", "p1")
        .add_file("file_server_early", "early")
        .commit()
        .await?;
    let head = CreateCommitContext::new(ctx, repo, vec![landed])
        .add_file("file_server", "server")
        .commit()
        .await?;
    Ok(Hop {
        fork,
        v1,
        v1_git,
        landed,
        head,
    })
}

async fn candidates(ctx: &CoreContext, repo: &TestRepo, hop: &Hop) -> Result<Vec<ChangesetId>> {
    RecordedPredecessorHop::new(repo)
        .candidates_for(ctx, hop.v1, hop.head, hop.fork)
        .await
}

#[mononoke::fbinit_test]
async fn hop_offers_the_recorded_successor_of_a_rewrite_naming_the_baseline(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = TestRepoFactory::new(fb)?.build().await?;
    let hop = hop(&ctx, &repo).await?;
    let v2 =
        commit_naming_predecessor(&ctx, &repo, hop.fork, "f", hop.v1_git, "rebase", None).await?;

    // A header on a commit the service never recorded is not read.
    assert!(candidates(&ctx, &repo, &hop).await?.is_empty());

    record_rewrite(&ctx, &repo, v2, hop.landed).await?;
    assert_eq!(candidates(&ctx, &repo, &hop).await?, vec![hop.landed]);
    Ok(())
}

#[mononoke::fbinit_test]
async fn hop_ignores_another_author_and_non_rewrite_ops(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = TestRepoFactory::new(fb)?.build().await?;
    let hop = hop(&ctx, &repo).await?;
    let forged = commit_naming_predecessor(
        &ctx,
        &repo,
        hop.fork,
        "f",
        hop.v1_git,
        "rebase",
        Some("mallory <mallory@example.com>"),
    )
    .await?;
    let picked =
        commit_naming_predecessor(&ctx, &repo, hop.fork, "g", hop.v1_git, "cherry-pick", None)
            .await?;
    record_rewrite(&ctx, &repo, forged, hop.landed).await?;
    record_rewrite(&ctx, &repo, picked, hop.head).await?;
    assert!(candidates(&ctx, &repo, &hop).await?.is_empty());
    Ok(())
}

#[mononoke::fbinit_test]
async fn hop_offers_every_eligible_candidate(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = TestRepoFactory::new(fb)?.build().await?;
    let hop = hop(&ctx, &repo).await?;
    let v2a =
        commit_naming_predecessor(&ctx, &repo, hop.fork, "f", hop.v1_git, "rebase", None).await?;
    let v2b =
        commit_naming_predecessor(&ctx, &repo, hop.fork, "g", hop.v1_git, "amend", None).await?;
    record_rewrite(&ctx, &repo, v2a, hop.landed).await?;
    record_rewrite(&ctx, &repo, v2b, hop.head).await?;
    let mut found = candidates(&ctx, &repo, &hop).await?;
    found.sort();
    let mut expected = vec![hop.landed, hop.head];
    expected.sort();
    assert_eq!(found, expected, "pushrebase decides that two is ambiguous");
    Ok(())
}

#[mononoke::fbinit_test]
async fn hop_skips_a_branch_longer_than_the_cap(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = TestRepoFactory::new(fb)?.build().await?;
    let mut hop = hop(&ctx, &repo).await?;
    let v2 =
        commit_naming_predecessor(&ctx, &repo, hop.fork, "f", hop.v1_git, "rebase", None).await?;
    record_rewrite(&ctx, &repo, v2, hop.landed).await?;
    for i in 0..PREDECESSOR_HOP_MAX_BRANCH_COMMITS {
        hop.head = CreateCommitContext::new(&ctx, &repo, vec![hop.head])
            .add_file(format!("file_{i}").as_str(), "x")
            .commit()
            .await?;
    }
    assert!(candidates(&ctx, &repo, &hop).await?.is_empty());
    Ok(())
}

#[mononoke::fbinit_test]
async fn hop_needs_the_baseline_git_identity(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = TestRepoFactory::new(fb)?.build().await?;
    let hop = hop(&ctx, &repo).await?;
    let unmapped = CreateCommitContext::new(&ctx, &repo, vec![hop.fork])
        .add_file("file_other", "other")
        .commit()
        .await?;
    let v2 =
        commit_naming_predecessor(&ctx, &repo, hop.fork, "f", hop.v1_git, "rebase", None).await?;
    record_rewrite(&ctx, &repo, v2, hop.landed).await?;
    let found = RecordedPredecessorHop::new(&repo)
        .candidates_for(&ctx, unmapped, hop.head, hop.fork)
        .await?;
    assert!(found.is_empty());
    Ok(())
}

#[mononoke::fbinit_test]
async fn client_label_uses_git_sha_and_revision(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = TestRepoFactory::new(fb)?.build().await?;
    let plain = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("f", "x")
        .commit()
        .await?;
    assert_eq!(
        describe_commit_for_client(&ctx, &repo, plain).await,
        plain.to_string(),
        "unmapped commits fall back to the bonsai id"
    );
    let reviewed = CreateCommitContext::new(&ctx, &repo, vec![plain])
        .add_file("g", "y")
        .set_message(
            "Title\n\nSummary\n\nDifferential Revision: https://www.internalfb.com/diff/D123",
        )
        .commit()
        .await?;
    let git = give_git_identity(&ctx, &repo, reviewed, 0x22).await?;
    assert_eq!(
        describe_commit_for_client(&ctx, &repo, reviewed).await,
        format!("{} (D123)", git.to_hex())
    );
    assert_eq!(differential_revision("no trailer"), None);
    Ok(())
}
