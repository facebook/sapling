/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

use anyhow::Result;
use anyhow::anyhow;
use bookmarks::BookmarkKey;
use bookmarks::BookmarksRef;
use commit_graph::CommitGraphRef;
use commit_id::CommitIdNames;
use commit_id::NamedCommitIdsArgs;
use commit_id::resolve_commit_id;
use context::CoreContext;
use futures::StreamExt;
use megarepolib::common::StackPosition;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use mononoke_types::ChangesetId;
use slog::info;

#[derive(Copy, Clone, Debug)]
struct GradualMergeProgressCommitIdNames;

impl CommitIdNames for GradualMergeProgressCommitIdNames {
    const NAMES: &'static [(&'static str, &'static str)] = &[
        (
            "pre-deletion-commit",
            "Include only descendants of the next commit",
        ),
        (
            "last-deletion-commit",
            "Exclude ancestors of the next commit",
        ),
    ];
}

#[derive(Debug, clap::Args)]
pub struct GradualMergeProgressArgs {
    #[clap(flatten)]
    pub repo_args: RepoArgs,

    #[clap(flatten)]
    commit_ids_args: NamedCommitIdsArgs<GradualMergeProgressCommitIdNames>,

    /// Bookmark to merge into
    #[clap(long)]
    target_bookmark: String,
}

pub async fn run(
    ctx: &CoreContext,
    app: MononokeApp,
    args: GradualMergeProgressArgs,
) -> Result<()> {
    let repo: Repo = app.open_repo(&args.repo_args).await?;

    let pre_deletion_commit = resolve_commit_id(
        ctx,
        &repo,
        args.commit_ids_args
            .named_commit_ids()
            .get("pre-deletion-commit")
            .ok_or(anyhow!("pre-deletion-commit is required"))?,
    )
    .await?;

    let last_deletion_commit = resolve_commit_id(
        ctx,
        &repo,
        args.commit_ids_args
            .named_commit_ids()
            .get("last-deletion-commit")
            .ok_or(anyhow!("last-deletion-commit is required"))?,
    )
    .await?;

    let bookmark_to_merge_into = BookmarkKey::new(&args.target_bookmark)?;

    let (merged_count, total_count) = gradual_merge_progress(
        ctx,
        &repo,
        &pre_deletion_commit,
        &last_deletion_commit,
        &bookmark_to_merge_into,
    )
    .await?;

    info!(
        ctx.logger(),
        "Progress: {}/{} commits merged", merged_count, total_count
    );

    Ok(())
}

/// Get total number of commits to merge and list
/// of commits that haven't been merged yet
async fn get_unmerged_commits_with_total_count(
    ctx: &CoreContext,
    repo: &Repo,
    pre_deletion_commit: &ChangesetId,
    last_deletion_commit: &ChangesetId,
    bookmark_to_merge_into: &BookmarkKey,
) -> Result<(usize, Vec<(ChangesetId, StackPosition)>)> {
    let commits_to_merge =
        find_all_commits_to_merge(ctx, repo, *pre_deletion_commit, *last_deletion_commit).await?;

    info!(
        ctx.logger(),
        "{} total commits to merge",
        commits_to_merge.len()
    );

    let commits_to_merge = commits_to_merge
        .into_iter()
        .enumerate()
        .map(|(idx, cs_id)| (cs_id, StackPosition(idx)))
        .collect::<Vec<_>>();

    let total_count = commits_to_merge.len();

    let unmerged_commits =
        find_unmerged_commits(ctx, repo, commits_to_merge, bookmark_to_merge_into).await?;

    Ok((total_count, unmerged_commits))
}

/// Get how many merges has been done and how many merges are there in total
pub async fn gradual_merge_progress(
    ctx: &CoreContext,
    repo: &Repo,
    pre_deletion_commit: &ChangesetId,
    last_deletion_commit: &ChangesetId,
    bookmark_to_merge_into: &BookmarkKey,
) -> Result<(usize, usize)> {
    let (to_merge_count, unmerged_commits) = get_unmerged_commits_with_total_count(
        ctx,
        repo,
        pre_deletion_commit,
        last_deletion_commit,
        bookmark_to_merge_into,
    )
    .await?;
    Ok((to_merge_count - unmerged_commits.len(), to_merge_count))
}

// Finds all commits that needs to be merged, regardless of whether they've been merged
// already or not.
async fn find_all_commits_to_merge(
    ctx: &CoreContext,
    repo: &Repo,
    pre_deletion_commit: ChangesetId,
    last_deletion_commit: ChangesetId,
) -> Result<Vec<ChangesetId>> {
    info!(ctx.logger(), "Finding all commits to merge...");
    let commits_to_merge = repo
        .commit_graph()
        .range_stream(ctx, pre_deletion_commit, last_deletion_commit)
        .await?
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .rev()
        .collect::<Vec<_>>();

    Ok(commits_to_merge)
}

// Out of all commits that need to be merged find commits that haven't been merged yet
async fn find_unmerged_commits(
    ctx: &CoreContext,
    repo: &Repo,
    mut commits_to_merge: Vec<(ChangesetId, StackPosition)>,
    bookmark_to_merge_into: &BookmarkKey,
) -> Result<Vec<(ChangesetId, StackPosition)>> {
    info!(
        ctx.logger(),
        "Finding commits that haven't been merged yet..."
    );
    let first = if let Some(first) = commits_to_merge.first() {
        first
    } else {
        // All commits has been merged already - nothing to do, just exit
        return Ok(vec![]);
    };

    // let bookmark_value = helpers::csid_resolve(ctx, repo, bookmark_to_merge_into).await?;
    let bookmark_value = repo
        .bookmarks()
        .get(
            ctx.clone(),
            bookmark_to_merge_into,
            bookmarks::Freshness::MostRecent,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("Bookmark {bookmark_to_merge_into} doesn't exist"))?;

    // Let's check if any commits has been merged already - to do that it's enough
    // to check if the first commit has been merged or not i.e. check if this commit
    // is ancestor of bookmark_to_merge_into or not.
    let has_merged_any = repo
        .commit_graph()
        .is_ancestor(ctx, first.0, bookmark_value)
        .await?;
    if !has_merged_any {
        return Ok(commits_to_merge);
    }

    // At this point we know that we've already merged at least a single commit.
    // Let's find the last commit that has been merged already.
    // To do that we do a bfs traversal starting from bookmark_to_merge_into, and
    // we stop as soon as we've found a commit from commits_to_merge list - because
    // we are merging starting from the top deletion commit, the latest merged commit
    // will be the closest to bookmark_to_merge_into
    //
    //   O <- bookmark_to_merge_into
    //   |
    //  ...
    //   M_B  <- we'll find this commit before M_A,  it's parent is commit B.
    //   |
    //  ...
    //   |
    //   M_A
    //   |
    //   O <- head where we want to merge
    //   |
    //   |      A - deletion commit, it was merged first in M_A
    //   O      |
    //   |      B - deletion commit, it was merged second in M_B
    //   O      |
    //          C <- it hasn't been merged yet
    //         ...
    //
    let mut commits_to_idx = HashMap::new();
    for (cs_id, stack_pos) in commits_to_merge.iter() {
        commits_to_idx.insert(*cs_id, *stack_pos);
    }

    let mut queue = VecDeque::new();
    queue.push_back(bookmark_value.clone());
    let mut visited = HashSet::new();
    visited.insert(bookmark_value);
    while let Some(cs_id) = queue.pop_back() {
        if let Some(found_idx) = commits_to_idx.get(&cs_id) {
            return Ok(commits_to_merge.split_off(found_idx.0 + 1));
        }

        let parents = repo.commit_graph().changeset_parents(ctx, cs_id).await?;
        for p in parents {
            if visited.insert(p) {
                queue.push_back(p);
            }
        }
    }

    Ok(commits_to_merge)
}
