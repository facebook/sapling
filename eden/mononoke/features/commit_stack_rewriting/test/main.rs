/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Result;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use commit_stack_rewriting::RewrittenCommit;
use commit_stack_rewriting::load_draft_stack;
use commit_stack_rewriting::rewrite_stack;
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::NonRootMPath;
use phases::Phases;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use tests_utils::CreateCommitContext;
use tests_utils::bookmark;

#[facet::container]
#[derive(Clone)]
struct TestRepo {
    #[facet]
    repo_identity: RepoIdentity,
    #[facet]
    repo_blobstore: RepoBlobstore,
    #[facet]
    commit_graph: CommitGraph,
    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,
    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,
    #[facet]
    bookmarks: dyn Bookmarks,
    #[facet]
    filestore_config: FilestoreConfig,
    #[facet]
    repo_derived_data: RepoDerivedData,
    #[facet]
    phases: dyn Phases,
}

fn ids(stack: &[mononoke_types::BonsaiChangeset]) -> Vec<ChangesetId> {
    stack.iter().map(|bcs| bcs.get_changeset_id()).collect()
}

#[mononoke::fbinit_test]
async fn load_stops_at_public_merge_and_excluded_commits(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let public = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "base")
        .commit()
        .await?;
    bookmark(&ctx, &repo, "heads/main")
        .create_publishing(public)
        .await?;
    let one = CreateCommitContext::new(&ctx, &repo, vec![public])
        .add_file("one", "one")
        .commit()
        .await?;
    let two = CreateCommitContext::new(&ctx, &repo, vec![one])
        .add_file("two", "two")
        .commit()
        .await?;

    let all: HashSet<ChangesetId> = [public, one, two].into_iter().collect();
    let stack = load_draft_stack(&ctx, &repo, two, |cs| all.contains(&cs)).await?;
    assert_eq!(ids(&stack), vec![one, two], "bottom first, public excluded");

    let only_top: HashSet<ChangesetId> = [two].into_iter().collect();
    let stack = load_draft_stack(&ctx, &repo, two, |cs| only_top.contains(&cs)).await?;
    assert_eq!(ids(&stack), vec![two]);

    let side = CreateCommitContext::new(&ctx, &repo, vec![public])
        .add_file("side", "side")
        .commit()
        .await?;
    let merge = CreateCommitContext::new(&ctx, &repo, vec![two, side])
        .commit()
        .await?;
    let stack = load_draft_stack(&ctx, &repo, merge, |_| true).await?;
    assert_eq!(ids(&stack), vec![merge], "a merge ends the chain");
    Ok(())
}

#[mononoke::fbinit_test]
async fn rewrite_replaces_messages_reparents_and_is_deterministic(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "base")
        .commit()
        .await?;
    let bottom = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("one", "one")
        .set_author("author <author@example.com>")
        .set_message("one, as pushed")
        .add_extra("mutpred", b"x".to_vec())
        .commit()
        .await?;
    let top = CreateCommitContext::new(&ctx, &repo, vec![bottom])
        .add_file_with_copy_info("copy", "one", (bottom, "one"))
        .set_message("two, as pushed")
        .commit()
        .await?;

    let stack = load_draft_stack(&ctx, &repo, top, |cs| cs != base).await?;
    let edit = |_: ChangesetId, bcs: &mut mononoke_types::BonsaiChangesetMut| {
        bcs.message = format!("{}\n\nReviewed By: r", bcs.message);
        Ok(())
    };
    let rewritten = rewrite_stack(&ctx, &repo, stack.clone(), edit).await?;
    assert_eq!(
        rewritten.iter().map(|r| r.original).collect::<Vec<_>>(),
        vec![bottom, top]
    );
    let new_bottom = rewritten[0].rewritten;
    let new_top = rewritten[1].rewritten;

    let bottom_bcs = new_bottom.load(&ctx, repo.repo_blobstore()).await?;
    assert_eq!(bottom_bcs.parents().collect::<Vec<_>>(), vec![base]);
    assert_eq!(bottom_bcs.message(), "one, as pushed\n\nReviewed By: r");
    assert_eq!(bottom_bcs.author(), "author <author@example.com>");
    assert!(bottom_bcs.hg_extra().all(|(k, _)| k != "mutpred"));

    let top_bcs = new_top.load(&ctx, repo.repo_blobstore()).await?;
    assert_eq!(top_bcs.parents().collect::<Vec<_>>(), vec![new_bottom]);
    let copy_from = match top_bcs.file_changes_map().get(&NonRootMPath::new("copy")?) {
        Some(FileChange::Change(tc)) => tc.copy_from().cloned(),
        other => panic!("expected a tracked change, got {other:?}"),
    };
    assert_eq!(copy_from, Some((NonRootMPath::new("one")?, new_bottom)));

    let again = rewrite_stack(&ctx, &repo, stack, edit).await?;
    assert_eq!(again, rewritten, "same input, same ids");
    Ok(())
}

#[mononoke::fbinit_test]
async fn rewrite_of_an_empty_stack_is_a_no_op(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let rewritten: Vec<RewrittenCommit> =
        rewrite_stack(&ctx, &repo, Vec::new(), |_, _| Ok(())).await?;
    assert!(rewritten.is_empty());
    Ok(())
}

#[mononoke::fbinit_test]
async fn rewrite_skips_commits_the_edit_leaves_unchanged(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let bottom = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("one", "one")
        .set_message("same")
        .commit()
        .await?;
    let top = CreateCommitContext::new(&ctx, &repo, vec![bottom])
        .add_file("two", "two")
        .set_message("changes")
        .commit()
        .await?;
    let stack = load_draft_stack(&ctx, &repo, top, |_| true).await?;
    let rewritten = rewrite_stack(&ctx, &repo, stack, |cs, bcs| {
        if cs == top {
            bcs.message = "changed".to_string();
        }
        Ok(())
    })
    .await?;
    assert_eq!(rewritten.len(), 1, "an identity rewrite records nothing");
    assert_eq!(rewritten[0].original, top);
    let top_bcs = rewritten[0]
        .rewritten
        .load(&ctx, repo.repo_blobstore())
        .await?;
    assert_eq!(top_bcs.parents().collect::<Vec<_>>(), vec![bottom]);
    Ok(())
}
