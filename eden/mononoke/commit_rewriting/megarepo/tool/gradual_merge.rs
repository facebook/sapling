/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use cmdlib::helpers;
use context::CoreContext;
use futures::compat::Stream01CompatExt;
use futures::TryStreamExt;
use maplit::hashset;
use megarepolib::common::create_and_save_bonsai;
use megarepolib::common::ChangesetArgs;
use megarepolib::common::ChangesetArgsFactory;
use megarepolib::common::StackPosition;
use mercurial_derived_data::DeriveHgChangeset;
use metaconfig_types::PushrebaseFlags;
use mononoke_api_types::InnerRepo;
use mononoke_types::ChangesetId;
use pushrebase::do_pushrebase_bonsai;
use reachabilityindex::LeastCommonAncestorsHint;
use revset::RangeNodeStream;
use slog::info;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

pub struct GradualMergeParams {
    pub pre_deletion_commit: ChangesetId,
    pub last_deletion_commit: ChangesetId,
    pub bookmark_to_merge_into: BookmarkName,
    pub merge_changeset_args_factory: Box<dyn ChangesetArgsFactory>,
    pub limit: Option<usize>,
    pub dry_run: bool,
}

/// Get total number of commits to merge and list
/// of commits that haven't been merged yet
async fn get_unmerged_commits_with_total_count(
    ctx: &CoreContext,
    repo: &InnerRepo,
    pre_deletion_commit: &ChangesetId,
    last_deletion_commit: &ChangesetId,
    bookmark_to_merge_into: &BookmarkName,
) -> Result<(usize, Vec<(ChangesetId, StackPosition)>), Error> {
    let commits_to_merge = find_all_commits_to_merge(
        ctx,
        &repo.blob_repo,
        *pre_deletion_commit,
        *last_deletion_commit,
    )
    .await?;

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
    repo: &InnerRepo,
    pre_deletion_commit: &ChangesetId,
    last_deletion_commit: &ChangesetId,
    bookmark_to_merge_into: &BookmarkName,
) -> Result<(usize, usize), Error> {
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

// This function implements a strategy to merge a large repository into another
// while avoiding sudden increase in the working copy size.
// Normally this function should be called after a list of deletion has been created
// (see pre_merge_delete functions). Each of these deletion commits decreases the
// size of the working copy, and so each of the merge adds just part of the working copy.
// See https://fburl.com/ujc6sv7x for more details.
//
//
//   M_B
//   |
//  ...
//   |
//   M_A
//   |
//   O <- head where we want to merge
//   |
//   |      A - deletion commit
//   O      |
//   |      B - deletion commit
//   O      |
//          O <- commit that we want to merge
//         ...
//
// M_A, M_B are merge commits that merge deletion commits. Note that the top deletion commit A
// is merged first, and deletion commit B is merged next.
pub async fn gradual_merge(
    ctx: &CoreContext,
    repo: &InnerRepo,
    params: &GradualMergeParams,
    pushrebase_flags: &PushrebaseFlags,
) -> Result<HashMap<ChangesetId, ChangesetId>, Error> {
    let GradualMergeParams {
        pre_deletion_commit,
        last_deletion_commit,
        bookmark_to_merge_into,
        merge_changeset_args_factory,
        limit,
        dry_run,
    } = params;

    let (_, unmerged_commits) = get_unmerged_commits_with_total_count(
        ctx,
        repo,
        pre_deletion_commit,
        last_deletion_commit,
        bookmark_to_merge_into,
    )
    .await?;

    let unmerged_commits = if let Some(limit) = limit {
        unmerged_commits.into_iter().take(*limit).collect()
    } else {
        unmerged_commits
    };
    info!(ctx.logger(), "merging {} commits", unmerged_commits.len());

    let mut res = HashMap::new();
    if !dry_run {
        for (cs_id, stack_pos) in unmerged_commits {
            let merge_changeset_args = merge_changeset_args_factory(stack_pos);
            let res_cs_id = push_merge_commit(
                ctx,
                &repo.blob_repo,
                cs_id,
                bookmark_to_merge_into,
                merge_changeset_args,
                pushrebase_flags,
            )
            .await?;

            res.insert(cs_id, res_cs_id);
        }
    } else {
        for (cs_id, stack_pos) in unmerged_commits {
            info!(
                ctx.logger(),
                "merging commits {}, with stack position {:?}", cs_id, stack_pos
            );
        }
    }

    Ok(res)
}

// Finds all commits that needs to be merged, regardless of whether they've been merged
// already or not.
async fn find_all_commits_to_merge(
    ctx: &CoreContext,
    repo: &BlobRepo,
    pre_deletion_commit: ChangesetId,
    last_deletion_commit: ChangesetId,
) -> Result<Vec<ChangesetId>, Error> {
    info!(ctx.logger(), "Finding all commits to merge...");
    let commits_to_merge = RangeNodeStream::new(
        ctx.clone(),
        repo.get_changeset_fetcher(),
        pre_deletion_commit,
        last_deletion_commit,
    )
    .compat()
    .try_collect::<Vec<_>>()
    .await?;

    Ok(commits_to_merge)
}

// Out of all commits that need to be merged find commits that haven't been merged yet
async fn find_unmerged_commits(
    ctx: &CoreContext,
    repo: &InnerRepo,
    mut commits_to_merge: Vec<(ChangesetId, StackPosition)>,
    bookmark_to_merge_into: &BookmarkName,
) -> Result<Vec<(ChangesetId, StackPosition)>, Error> {
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

    let bookmark_value =
        helpers::csid_resolve(ctx, &repo.blob_repo, bookmark_to_merge_into).await?;

    // Let's check if any commits has been merged already - to do that it's enough
    // to check if the first commit has been merged or not i.e. check if this commit
    // is ancestor of bookmark_to_merge_into or not.
    let has_merged_any = repo
        .skiplist_index
        .is_ancestor(
            ctx,
            &repo.blob_repo.get_changeset_fetcher(),
            first.0,
            bookmark_value,
        )
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

        let parents = repo
            .blob_repo
            .get_changeset_parents_by_bonsai(ctx.clone(), cs_id)
            .await?;
        for p in parents {
            if visited.insert(p) {
                queue.push_back(p);
            }
        }
    }

    Ok(commits_to_merge)
}

// Pushrebase a single merge commit
async fn push_merge_commit(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id_to_merge: ChangesetId,
    bookmark_to_merge_into: &BookmarkName,
    merge_changeset_args: ChangesetArgs,
    pushrebase_flags: &PushrebaseFlags,
) -> Result<ChangesetId, Error> {
    info!(ctx.logger(), "Preparing to merge {}", cs_id_to_merge);
    let bookmark_value = helpers::csid_resolve(ctx, repo, bookmark_to_merge_into).await?;

    let merge_cs_id = create_and_save_bonsai(
        ctx,
        repo,
        vec![bookmark_value, cs_id_to_merge],
        Default::default(),
        merge_changeset_args,
    )
    .await?;

    info!(ctx.logger(), "Created merge changeset {}", merge_cs_id);

    // Generating hg changeset from bonsai changeset will give us a validation
    // that this merge commit is correct
    let merge_hg_cs_id = repo.derive_hg_changeset(ctx, merge_cs_id).await?;

    info!(ctx.logger(), "Generated hg changeset {}", merge_hg_cs_id);
    info!(ctx.logger(), "Now running pushrebase...");

    let merge_cs = merge_cs_id.load(ctx, repo.blobstore()).await?;
    let pushrebase_res = do_pushrebase_bonsai(
        ctx,
        repo,
        pushrebase_flags,
        bookmark_to_merge_into,
        &hashset![merge_cs],
        &[],
    )
    .await?;

    info!(ctx.logger(), "Pushrebased to {}", pushrebase_res.head);
    Ok(pushrebase_res.head)
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use mononoke_types::DateTime;
    use mononoke_types::MPath;
    use tests_utils::bookmark;
    use tests_utils::drawdag::create_from_dag;
    use tests_utils::list_working_copy_utf8;
    use tests_utils::CreateCommitContext;

    #[fbinit::test]
    async fn test_find_all_commits_to_merge(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (repo, pre_deletion_commit, deletion_commits) = create_repo(&ctx).await?;
        let commits_to_merge = find_all_commits_to_merge(
            &ctx,
            &repo.blob_repo,
            pre_deletion_commit,
            *deletion_commits.last().unwrap(),
        )
        .await?;
        let mut expected = vec![pre_deletion_commit];
        expected.extend(deletion_commits);
        expected.reverse();
        assert_eq!(commits_to_merge, expected);
        Ok(())
    }

    #[fbinit::test]
    async fn test_find_unmerged_commits(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (repo, pre_deletion_commit, deletion_commits) = create_repo(&ctx).await?;
        let commits_to_merge = find_all_commits_to_merge(
            &ctx,
            &repo.blob_repo,
            pre_deletion_commit,
            *deletion_commits.last().unwrap(),
        )
        .await?;

        let commits_to_merge = commits_to_merge
            .into_iter()
            .enumerate()
            .map(|(idx, cs_id)| (cs_id, StackPosition(idx)))
            .collect::<Vec<_>>();
        let head = BookmarkName::new("head")?;
        let unmerged_commits =
            find_unmerged_commits(&ctx, &repo, commits_to_merge.clone(), &head).await?;

        assert_eq!(commits_to_merge, unmerged_commits);

        // Now merge a single commit into "head"
        let head_value = helpers::csid_resolve(&ctx, &repo.blob_repo, head.clone()).await?;

        let merge = CreateCommitContext::new(
            &ctx,
            &repo.blob_repo,
            vec![head_value, unmerged_commits.first().unwrap().0],
        )
        .commit()
        .await?;
        bookmark(&ctx, &repo.blob_repo, "head")
            .set_to(merge)
            .await?;

        let unmerged_commits =
            find_unmerged_commits(&ctx, &repo, commits_to_merge.clone(), &head).await?;
        let mut expected = commits_to_merge.clone();
        expected.remove(0);
        assert_eq!(expected, unmerged_commits);

        // Merge next commit into head
        let head_value = helpers::csid_resolve(&ctx, &repo.blob_repo, head.clone()).await?;

        let merge = CreateCommitContext::new(
            &ctx,
            &repo.blob_repo,
            vec![head_value, unmerged_commits.first().unwrap().0],
        )
        .commit()
        .await?;
        bookmark(&ctx, &repo.blob_repo, "head")
            .set_to(merge)
            .await?;

        let unmerged_commits =
            find_unmerged_commits(&ctx, &repo, commits_to_merge.clone(), &head).await?;
        let mut expected = commits_to_merge.clone();
        expected.remove(0);
        expected.remove(0);
        assert_eq!(expected, unmerged_commits);

        Ok(())
    }

    #[fbinit::test]
    async fn test_gradual_merge(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (repo, pre_deletion_commit, deletion_commits) = create_repo(&ctx).await?;

        let args_factory = get_default_merge_args_factory();
        let mut params = GradualMergeParams {
            pre_deletion_commit,
            last_deletion_commit: *deletion_commits.last().unwrap(),
            bookmark_to_merge_into: BookmarkName::new("head")?,
            merge_changeset_args_factory: args_factory,
            limit: None,
            dry_run: false,
        };

        let pushrebase_flags = {
            let mut flags = PushrebaseFlags::default();
            flags.rewritedates = true;
            flags.forbid_p2_root_rebases = true;
            flags.casefolding_check = true;
            flags.recursion_limit = None;
            flags
        };

        // Test dry-run mode
        params.dry_run = true;
        let merged = gradual_merge(&ctx, &repo, &params, &pushrebase_flags).await?;
        assert!(merged.is_empty());

        params.dry_run = false;
        let merged = gradual_merge(&ctx, &repo, &params, &pushrebase_flags).await?;
        verify_gradual_merges(
            &ctx,
            &repo.blob_repo,
            merged,
            pre_deletion_commit,
            &deletion_commits,
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_gradual_merge_with_limit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (repo, pre_deletion_commit, deletion_commits) = create_repo(&ctx).await?;

        let args_factory = get_default_merge_args_factory();
        let params = GradualMergeParams {
            pre_deletion_commit,
            last_deletion_commit: *deletion_commits.last().unwrap(),
            bookmark_to_merge_into: BookmarkName::new("head")?,
            merge_changeset_args_factory: args_factory,
            limit: Some(1),
            dry_run: false,
        };

        let pushrebase_flags = {
            let mut flags = PushrebaseFlags::default();
            flags.rewritedates = true;
            flags.forbid_p2_root_rebases = true;
            flags.casefolding_check = true;
            flags.recursion_limit = None;
            flags
        };

        let mut result = HashMap::new();
        for _ in 0..3 {
            let merged = gradual_merge(&ctx, &repo, &params, &pushrebase_flags).await?;
            assert_eq!(merged.len(), 1);
            result.extend(merged)
        }
        verify_gradual_merges(
            &ctx,
            &repo.blob_repo,
            result,
            pre_deletion_commit,
            &deletion_commits,
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_stack_position(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (repo, pre_deletion_commit, deletion_commits) = create_repo(&ctx).await?;
        let args_factory = Box::new(|stack_pos: StackPosition| ChangesetArgs {
            author: "author".to_string(),
            message: format!("{}", stack_pos.0),
            datetime: DateTime::now(),
            bookmark: None,
            mark_public: false,
        });

        let params = GradualMergeParams {
            pre_deletion_commit,
            last_deletion_commit: *deletion_commits.last().unwrap(),
            bookmark_to_merge_into: BookmarkName::new("head")?,
            merge_changeset_args_factory: args_factory,
            limit: Some(1),
            dry_run: false,
        };

        let pushrebase_flags = {
            let mut flags = PushrebaseFlags::default();
            flags.rewritedates = true;
            flags.forbid_p2_root_rebases = true;
            flags.casefolding_check = true;
            flags.recursion_limit = None;
            flags
        };

        let mut result = HashMap::new();
        for i in 0..3 {
            let merged = gradual_merge(&ctx, &repo, &params, &pushrebase_flags).await?;
            assert_eq!(merged.len(), 1);
            let pushrebased_cs_id = merged.values().next().unwrap();
            let bcs_id = pushrebased_cs_id
                .load(&ctx, repo.blob_repo.blobstore())
                .await?;
            assert_eq!(bcs_id.message(), format!("{}", i));
            result.extend(merged)
        }
        verify_gradual_merges(
            &ctx,
            &repo.blob_repo,
            result,
            pre_deletion_commit,
            &deletion_commits,
        )
        .await?;

        Ok(())
    }

    async fn create_repo(
        ctx: &CoreContext,
    ) -> Result<(InnerRepo, ChangesetId, Vec<ChangesetId>), Error> {
        let repo: InnerRepo = test_repo_factory::build_empty(ctx.fb)?;

        let dag = create_from_dag(
            ctx,
            &repo.blob_repo,
            r##"
               A-B-C

               D-E-F
             "##,
        )
        .await?;
        let pre_deletion_commit = *dag.get("F").unwrap();

        // Create deletion commits
        let first_deletion_commit =
            CreateCommitContext::new(ctx, &repo.blob_repo, vec![pre_deletion_commit])
                .delete_file("F")
                .commit()
                .await?;

        let second_deletion_commit =
            CreateCommitContext::new(ctx, &repo.blob_repo, vec![first_deletion_commit])
                .delete_file("E")
                .commit()
                .await?;

        bookmark(ctx, &repo.blob_repo, "head")
            .set_to(*dag.get("C").unwrap())
            .await?;

        Ok((
            repo,
            pre_deletion_commit,
            vec![first_deletion_commit, second_deletion_commit],
        ))
    }

    async fn verify_gradual_merges(
        ctx: &CoreContext,
        repo: &BlobRepo,
        gradual_merge_result: HashMap<ChangesetId, ChangesetId>,
        pre_deletion_commit: ChangesetId,
        deletion_commits: &Vec<ChangesetId>,
    ) -> Result<(), Error> {
        assert_eq!(gradual_merge_result.len(), 3);
        let working_copy = list_working_copy_utf8(
            ctx,
            &repo,
            *gradual_merge_result.get(&pre_deletion_commit).unwrap(),
        )
        .await?;
        assert_eq!(
            hashmap! {
                MPath::new("A")? => "A".to_string(),
                MPath::new("B")? => "B".to_string(),
                MPath::new("C")? => "C".to_string(),
                MPath::new("D")? => "D".to_string(),
                MPath::new("E")? => "E".to_string(),
                MPath::new("F")? => "F".to_string(),
            },
            working_copy
        );

        let working_copy = list_working_copy_utf8(
            ctx,
            &repo,
            *gradual_merge_result.get(&deletion_commits[0]).unwrap(),
        )
        .await?;
        assert_eq!(
            hashmap! {
                MPath::new("A")? => "A".to_string(),
                MPath::new("B")? => "B".to_string(),
                MPath::new("C")? => "C".to_string(),
                MPath::new("D")? => "D".to_string(),
                MPath::new("E")? => "E".to_string(),
            },
            working_copy
        );

        let working_copy = list_working_copy_utf8(
            ctx,
            &repo,
            *gradual_merge_result.get(&deletion_commits[1]).unwrap(),
        )
        .await?;
        assert_eq!(
            hashmap! {
                MPath::new("A")? => "A".to_string(),
                MPath::new("B")? => "B".to_string(),
                MPath::new("C")? => "C".to_string(),
                MPath::new("D")? => "D".to_string(),
            },
            working_copy
        );

        for merge_cs_id in gradual_merge_result.values() {
            let hg_cs_id = repo.derive_hg_changeset(ctx, *merge_cs_id).await?;
            let hg_cs = hg_cs_id.load(ctx, repo.blobstore()).await?;
            assert!(hg_cs.files().is_empty());
        }
        Ok(())
    }

    fn get_default_merge_args_factory() -> Box<dyn ChangesetArgsFactory> {
        Box::new(|_| ChangesetArgs {
            author: "author".to_string(),
            message: "merge".to_string(),
            datetime: DateTime::now(),
            bookmark: None,
            mark_public: false,
        })
    }
}
