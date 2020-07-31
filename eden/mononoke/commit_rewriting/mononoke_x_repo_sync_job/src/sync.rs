/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::reporting::{
    log_bookmark_deletion_result, log_non_pushrebase_sync_single_changeset_result,
    log_pushrebase_sync_single_changeset_result,
};
use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::{BookmarkName, BookmarkUpdateLogEntry, BookmarkUpdateReason};
use cloned::cloned;
use context::CoreContext;
use cross_repo_sync::{
    find_toposorted_unsynced_ancestors,
    types::{Source, Target},
    CommitSyncOutcome, CommitSyncer,
};
use futures::{
    compat::Future01CompatExt,
    future::{try_join_all, TryFutureExt},
    try_join,
};
use futures_old::{stream::Stream, Future};
use futures_stats::TimedFutureExt;
use mononoke_types::ChangesetId;
use reachabilityindex::ReachabilityIndex;
use revset::DifferenceOfUnionsOfAncestorsNodeStream;
use scuba_ext::ScubaSampleBuilder;
use skiplist::SkiplistIndex;
use slog::info;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use synced_commit_mapping::SyncedCommitMapping;

/// Sync all new commits and update the bookmark that were introduced by BookmarkUpdateLogEntry
/// in the source repo.
/// This function:
/// 1) Finds commits that needs syncing
/// 2) Syncs them from source repo into target (*)
/// 3) Updates the bookmark
///
/// (*) There are two ways how a commit can be synced from source repo into a target repo.
/// It can either be rewritten and saved into a target repo, or rewritten and pushrebased
/// in a target repo. This depends on which bookmark introduced a commit - if it's a
/// common_pushrebase_bookmark (usually "master"), then a commit will be pushrebased.
/// Otherwise it will be synced without pushrebase.
pub async fn sync_single_bookmark_update_log<M: SyncedCommitMapping + Clone + 'static>(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncer<M>,
    entry: BookmarkUpdateLogEntry,
    source_skiplist_index: &Source<Arc<SkiplistIndex>>,
    target_skiplist_index: &Target<Arc<SkiplistIndex>>,
    common_pushrebase_bookmarks: &HashSet<BookmarkName>,
    mut scuba_sample: ScubaSampleBuilder,
) -> Result<Vec<ChangesetId>, Error> {
    info!(ctx.logger(), "processing log entry #{}", entry.id);
    let bookmark = commit_syncer.get_bookmark_renamer()(&entry.bookmark_name)
        .ok_or(format_err!("unexpected empty bookmark rename"))?;
    scuba_sample
        .add("source_bookmark_name", format!("{}", entry.bookmark_name))
        .add("target_bookmark_name", format!("{}", bookmark));

    let to_cs_id = match entry.to_changeset_id {
        Some(to_cs_id) => to_cs_id,
        None => {
            // This is a bookmark deletion - just delete a bookmark and exit,
            // no need to sync commits
            process_bookmark_deletion(
                ctx,
                commit_syncer,
                scuba_sample,
                &bookmark,
                common_pushrebase_bookmarks,
            )
            .await?;

            return Ok(vec![]);
        }
    };

    // Find commits that needs syncing
    let unsynced_ancestors =
        find_toposorted_unsynced_ancestors(&ctx, &commit_syncer, to_cs_id.clone()).await?;
    let len = unsynced_ancestors.len();
    info!(ctx.logger(), "{} unsynced ancestors of {}", len, to_cs_id);

    if common_pushrebase_bookmarks.contains(&bookmark) {
        // This is a commit that was introduced by common pushrebase bookmark (e.g. "master").
        // Use pushrebase to sync a commit.
        if let Some(from_cs_id) = entry.from_changeset_id {
            check_forward_move(
                ctx,
                commit_syncer,
                &source_skiplist_index.0,
                to_cs_id,
                from_cs_id,
            )
            .await?;
        }

        sync_commits_via_pushrebase(
            &ctx,
            &commit_syncer,
            &source_skiplist_index,
            &target_skiplist_index,
            &bookmark,
            &common_pushrebase_bookmarks,
            scuba_sample.clone(),
            unsynced_ancestors,
        )
        .await
    } else {
        // Use a normal sync since a bookmark is not a common pushrebase bookmark
        let mut res = vec![];
        for cs_id in unsynced_ancestors {
            let synced = sync_commit_without_pushrebase(
                &ctx,
                &commit_syncer,
                &target_skiplist_index,
                scuba_sample.clone(),
                cs_id,
                &common_pushrebase_bookmarks,
            )
            .await?;
            res.extend(synced);
        }
        let maybe_remapped_cs_id = find_remapped_cs_id(ctx, commit_syncer, to_cs_id).await?;
        let remapped_cs_id =
            maybe_remapped_cs_id.ok_or(format_err!("unknown sync outcome for {}", to_cs_id))?;
        force_set_bookmark(
            &ctx,
            &commit_syncer.get_target_repo(),
            &bookmark,
            remapped_cs_id,
        )
        .await?;
        Ok(res)
    }
    // TODO(stash): test with other movers
    // Note: counter update might fail after a successful sync
}

/// This function syncs commits via pushrebase with a caveat - some commits shouldn't be
/// pushrebased! Consider pushing of a merge
///
///  source repo (X - synced commit, O - unsynced commit)
///
///     O <- merge commit (this commit needs to be pushrebased in target repo)
///    / |
///   X   O <- this commit DOES NOT NEED to be pushrebased in the target repo
///  ...  |
///      ...
///
/// Just as normal pushrebase behaves while pushing merges, we rebase the actual merge
/// commit and it's ancestors, but we don't rebase merge ancestors.
pub async fn sync_commits_via_pushrebase<M: SyncedCommitMapping + Clone + 'static>(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncer<M>,
    source_skiplist_index: &Source<Arc<SkiplistIndex>>,
    target_skiplist_index: &Target<Arc<SkiplistIndex>>,
    bookmark: &BookmarkName,
    common_pushrebase_bookmarks: &HashSet<BookmarkName>,
    scuba_sample: ScubaSampleBuilder,
    unsynced_ancestors: Vec<ChangesetId>,
) -> Result<Vec<ChangesetId>, Error> {
    let source_repo = commit_syncer.get_source_repo();
    // It stores commits that were introduced as part of current bookmark update, but that
    // shouldn't be pushrebased.
    let mut no_pushrebase = HashSet::new();
    let mut res = vec![];

    // Iterate in reverse order i.e. descendants before ancestors
    for cs_id in unsynced_ancestors.iter().rev() {
        if no_pushrebase.contains(cs_id) {
            continue;
        }

        let bcs = cs_id.load(ctx.clone(), source_repo.blobstore()).await?;

        let mut parents = bcs.parents();
        let maybe_p1 = parents.next();
        let maybe_p2 = parents.next();
        if let (Some(p1), Some(p2)) = (maybe_p1, maybe_p2) {
            if parents.next().is_some() {
                return Err(format_err!("only 2 parent merges are supported"));
            }

            no_pushrebase.extend(
                validate_if_new_repo_merge(
                    &ctx,
                    &source_repo,
                    source_skiplist_index.clone(),
                    p1,
                    p2,
                )
                .await?,
            );
        }
    }

    for cs_id in unsynced_ancestors {
        let maybe_new_cs_id = if no_pushrebase.contains(&cs_id) {
            sync_commit_without_pushrebase(
                &ctx,
                commit_syncer,
                target_skiplist_index,
                scuba_sample.clone(),
                cs_id,
                common_pushrebase_bookmarks,
            )
            .await?
        } else {
            info!(
                ctx.logger(),
                "syncing {} via pushrebase for {}", cs_id, bookmark
            );
            let (stats, result) = pushrebase_commit(ctx.clone(), &commit_syncer, &bookmark, cs_id)
                .timed()
                .await;
            log_pushrebase_sync_single_changeset_result(
                ctx.clone(),
                scuba_sample.clone(),
                cs_id,
                &result,
                stats,
            );
            let maybe_new_cs_id = result?;
            maybe_new_cs_id.into_iter().collect()
        };

        res.extend(maybe_new_cs_id);
    }
    Ok(res)
}

pub async fn sync_commit_without_pushrebase<M: SyncedCommitMapping + Clone + 'static>(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncer<M>,
    target_skiplist_index: &Target<Arc<SkiplistIndex>>,
    scuba_sample: ScubaSampleBuilder,
    cs_id: ChangesetId,
    common_pushrebase_bookmarks: &HashSet<BookmarkName>,
) -> Result<Vec<ChangesetId>, Error> {
    info!(ctx.logger(), "syncing {}", cs_id);
    let bcs = cs_id
        .load(ctx.clone(), commit_syncer.get_source_repo().blobstore())
        .await?;

    let (stats, result) = if bcs.is_merge() {
        // We allow syncing of a merge only if there's no intersection between ancestors of this
        // merge commit and ancestors of common pushrebase bookmark in target repo.
        // The code below does exactly that - it fetches common_pushrebase_bookmarks and parent
        // commits from the target repo, and then it checks if there are no intersection.
        let target_repo = commit_syncer.get_target_repo();
        let mut book_values = vec![];
        for common_bookmark in common_pushrebase_bookmarks {
            book_values.push(
                target_repo
                    .get_bonsai_bookmark(ctx.clone(), common_bookmark)
                    .compat(),
            );
        }

        let book_values = try_join_all(book_values).await?;
        let book_values = book_values.into_iter().filter_map(|x| x).collect();

        let parents = try_join_all(
            bcs.parents()
                .map(|p| find_remapped_cs_id(&ctx, &commit_syncer, p)),
        )
        .await?;
        let maybe_independent_branch = check_if_independent_branch_and_return(
            &ctx,
            &target_repo,
            target_skiplist_index.0.clone(),
            parents.into_iter().filter_map(|x| x).collect(),
            book_values,
        )
        .await?;

        // Merge is from a branch completely independent from common_pushrebase_bookmark -
        // it's fine to sync it.
        if maybe_independent_branch.is_some() {
            commit_syncer
                .unsafe_naive_sync_commit(ctx.clone(), cs_id)
                .timed()
                .await
        } else {
            return Err(format_err!(
                "cannot sync merge commit - one of it's ancestors is an ancestor of a common pushrebase bookmark"
            ));
        }
    } else {
        commit_syncer
            .unsafe_sync_commit(ctx.clone(), cs_id)
            .timed()
            .await
    };

    log_non_pushrebase_sync_single_changeset_result(
        ctx.clone(),
        scuba_sample.clone(),
        cs_id,
        &result,
        stats,
    );

    let maybe_cs_id = result?;
    Ok(maybe_cs_id.into_iter().collect())
}

async fn process_bookmark_deletion<M: SyncedCommitMapping + Clone + 'static>(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncer<M>,
    scuba_sample: ScubaSampleBuilder,
    bookmark: &BookmarkName,
    common_pushrebase_bookmarks: &HashSet<BookmarkName>,
) -> Result<(), Error> {
    if common_pushrebase_bookmarks.contains(&bookmark) {
        Err(format_err!(
            "unexpected deletion of a shared bookmark {}",
            bookmark
        ))
    } else {
        info!(ctx.logger(), "deleting bookmark {}", bookmark);
        let (stats, result) =
            force_delete_bookmark(ctx.clone(), &commit_syncer.get_target_repo(), &bookmark)
                .timed()
                .await;
        log_bookmark_deletion_result(scuba_sample, &result, stats);
        result
    }
}

async fn check_forward_move<M: SyncedCommitMapping + Clone + 'static>(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncer<M>,
    skiplist_index: &Arc<SkiplistIndex>,
    to_cs_id: ChangesetId,
    from_cs_id: ChangesetId,
) -> Result<(), Error> {
    if !skiplist_index
        .query_reachability(
            ctx,
            &commit_syncer.get_source_repo().get_changeset_fetcher(),
            to_cs_id,
            from_cs_id,
        )
        .await?
    {
        return Err(format_err!(
            "non-forward moves of shared bookmarks are not allowed"
        ));
    }
    Ok(())
}

async fn find_remapped_cs_id<M: SyncedCommitMapping + Clone + 'static>(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncer<M>,
    orig_cs_id: ChangesetId,
) -> Result<Option<ChangesetId>, Error> {
    let maybe_sync_outcome = commit_syncer
        .get_commit_sync_outcome(ctx.clone(), orig_cs_id)
        .await?;
    use CommitSyncOutcome::*;
    match maybe_sync_outcome {
        Some(RewrittenAs(cs_id, _)) | Some(EquivalentWorkingCopyAncestor(cs_id)) => Ok(Some(cs_id)),
        Some(Preserved) => Ok(Some(orig_cs_id)),
        Some(NotSyncCandidate) => Err(format_err!("unexpected NotSyncCandidate")),
        None => Ok(None),
    }
}

pub async fn pushrebase_commit<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
    bookmark: &BookmarkName,
    cs_id: ChangesetId,
) -> Result<Option<ChangesetId>, Error> {
    let source_repo = commit_syncer.get_source_repo();
    let bcs = cs_id.load(ctx.clone(), source_repo.blobstore()).await?;

    commit_syncer
        .unsafe_sync_commit_pushrebase(ctx, bcs, bookmark.clone())
        .await
}

/// Function validates if a this merge is supported for x-repo sync. At the moment we support
/// only a single type of merges - merge that introduces a new repo i.e. merge p1 and p2
/// have no shared history.
///
///     O <- merge commit to sync
///    / |
///   O   O <- these are new commits we need to sync
///   |   |
///   |   ...
///
/// This function returns new commits that were introduced by this merge
async fn validate_if_new_repo_merge(
    ctx: &CoreContext,
    repo: &BlobRepo,
    skiplist_index: Source<Arc<SkiplistIndex>>,
    p1: ChangesetId,
    p2: ChangesetId,
) -> Result<Vec<ChangesetId>, Error> {
    let p1gen = repo.get_generation_number(ctx.clone(), p1).compat();
    let p2gen = repo.get_generation_number(ctx.clone(), p2).compat();
    let (p1gen, p2gen) = try_join!(p1gen, p2gen)?;
    // FIXME: this code has an assumption that parent with a smaller generation number is a
    // parent that introduces a new repo. This is usually the case, however it might not be true
    // in some rare cases.
    let (larger_gen, smaller_gen) = if p1gen > p2gen { (p1, p2) } else { (p2, p1) };

    let err_msg = || format_err!("unsupported merge - only merges of new repos are supported");

    // Check if this is a diamond merge i.e. check if any of the ancestor of smaller_gen
    // is also ancestor of larger_gen.
    let maybe_independent_branch = check_if_independent_branch_and_return(
        &ctx,
        &repo,
        skiplist_index.0,
        vec![smaller_gen],
        vec![larger_gen],
    )
    .await?;

    let independent_branch = maybe_independent_branch.ok_or_else(err_msg)?;

    Ok(independent_branch)
}

/// Checks if `branch_tips` and their ancestors have no intersection with ancestors of
/// other_branches. If there are no intersection then branch_tip and it's ancestors are returned,
/// i.e. (::branch_tips) is returned in mercurial's revset terms
async fn check_if_independent_branch_and_return(
    ctx: &CoreContext,
    repo: &BlobRepo,
    skiplist_index: Arc<SkiplistIndex>,
    branch_tips: Vec<ChangesetId>,
    other_branches: Vec<ChangesetId>,
) -> Result<Option<Vec<ChangesetId>>, Error> {
    let fetcher = repo.get_changeset_fetcher();
    let bcss = DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
        ctx.clone(),
        &fetcher,
        skiplist_index,
        branch_tips.clone(),
        other_branches,
    )
    .map({
        cloned!(ctx, repo);
        move |cs| cs.load(ctx.clone(), repo.blobstore()).compat().from_err()
    })
    .buffered(100)
    .collect()
    .compat()
    .await?;

    let bcss: Vec<_> = bcss.into_iter().rev().collect();
    let mut cs_to_parents: HashMap<_, Vec<_>> = HashMap::new();
    for bcs in &bcss {
        let cs_id = bcs.get_changeset_id();
        cs_to_parents.insert(cs_id, bcs.parents().collect());
    }

    // If any of branch_tips hasn't been returned, then it was an ancestor of some of the
    // other_branches.
    for tip in branch_tips {
        if !cs_to_parents.contains_key(&tip) {
            return Ok(None);
        }
    }

    for parents in cs_to_parents.values() {
        for p in parents {
            if !cs_to_parents.contains_key(p) {
                return Ok(None);
            }
        }
    }

    Ok(Some(cs_to_parents.keys().cloned().collect()))
}

async fn force_delete_bookmark(
    ctx: CoreContext,
    repo: &BlobRepo,
    bookmark: &BookmarkName,
) -> Result<(), Error> {
    let mut book_txn = repo.update_bookmark_transaction(ctx);
    book_txn.force_delete(&bookmark, BookmarkUpdateReason::XRepoSync, None)?;
    book_txn.commit().await?;

    Ok(())
}

async fn force_set_bookmark(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark: &BookmarkName,
    cs_id: ChangesetId,
) -> Result<(), Error> {
    let mut book_txn = repo.update_bookmark_transaction(ctx.clone());
    book_txn.force_set(&bookmark, cs_id, BookmarkUpdateReason::XRepoSync, None)?;
    book_txn.commit().await?;

    Ok(())
}

pub fn is_already_synced<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    cs_id: ChangesetId,
    commit_syncer: CommitSyncer<M>,
) -> impl Future<Item = bool, Error = Error> {
    let source_repo_id = commit_syncer.get_source_repo().get_repoid();
    let target_repo_id = commit_syncer.get_target_repo().get_repoid();
    info!(
        ctx.logger(),
        "Checking if {} is already synced {}->{}",
        cs_id,
        source_repo_id.id(),
        target_repo_id.id()
    );
    commit_syncer
        .get_mapping()
        .get(ctx.clone(), source_repo_id, cs_id.clone(), target_repo_id)
        .map({
            cloned!(ctx);
            move |maybe_mapping| {
                if maybe_mapping.is_some() {
                    info!(ctx.logger(), "{} is already synced", cs_id);
                    true
                } else {
                    false
                }
            }
        })
}

#[cfg(test)]
mod test {
    use super::*;
    use bookmarks::Freshness;
    use cross_repo_sync::validation;
    use cross_repo_sync_test_utils::init_small_large_repo;
    use fbinit::FacebookInit;
    use futures_old::stream::Stream;
    use maplit::hashset;
    use mutable_counters::MutableCounters;
    use tests_utils::{bookmark, resolve_cs_id, CreateCommitContext};

    use mutable_counters::SqlMutableCounters;
    use sql_construct::SqlConstruct;
    use synced_commit_mapping::SqlSyncedCommitMapping;
    use tokio_compat::runtime::Runtime;

    #[fbinit::test]
    fn test_simple(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new()?;
        runtime.block_on_std(async move {
            let mutable_counters = SqlMutableCounters::with_sqlite_in_memory().unwrap();

            let ctx = CoreContext::test_mock(fb);
            let (syncers, _) = init_small_large_repo(&ctx).await?;
            let commit_syncer = syncers.small_to_large;
            let smallrepo = commit_syncer.get_source_repo();

            // Single commit
            let new_master = CreateCommitContext::new(&ctx, &smallrepo, vec!["master"])
                .add_file("newfile", "newcontent")
                .commit()
                .await?;
            bookmark(&ctx, &smallrepo, "master")
                .set_to(new_master)
                .await?;

            sync_and_validate(&ctx, &commit_syncer, &mutable_counters).await?;

            let non_master_commit = CreateCommitContext::new(&ctx, &smallrepo, vec!["master"])
                .add_file("nonmasterfile", "nonmastercontent")
                .commit()
                .await?;
            bookmark(&ctx, &smallrepo, "nonmasterbookmark")
                .set_to(non_master_commit)
                .await?;

            sync_and_validate(&ctx, &commit_syncer, &mutable_counters).await?;

            // Create a stack of commits
            let first_in_stack = CreateCommitContext::new(&ctx, &smallrepo, vec!["master"])
                .add_file("stack", "first")
                .commit()
                .await?;

            let second_in_stack = CreateCommitContext::new(&ctx, &smallrepo, vec![first_in_stack])
                .add_file("stack", "second")
                .commit()
                .await?;
            bookmark(&ctx, &smallrepo, "master")
                .set_to(second_in_stack)
                .await?;

            // Create a commit that should NOT be rewritten because it's based on a pre-move
            // commit
            let premove = CreateCommitContext::new(&ctx, &smallrepo, vec!["premove"])
                .add_file("premove", "premovecontent")
                .commit()
                .await?;
            bookmark(&ctx, &smallrepo, "newpremove")
                .set_to(premove)
                .await?;

            // Move a bookmark
            bookmark(&ctx, &smallrepo, "newpremove")
                .set_to("premove")
                .await?;
            sync_and_validate(&ctx, &commit_syncer, &mutable_counters).await?;
            let commit_sync_outcome = commit_syncer
                .get_commit_sync_outcome(ctx.clone(), premove)
                .await?
                .ok_or(format_err!("commit sync outcome not set"))?;
            assert_eq!(commit_sync_outcome, CommitSyncOutcome::Preserved);

            // Delete bookmarks
            bookmark(&ctx, &smallrepo, "newpremove").delete().await?;
            bookmark(&ctx, &smallrepo, "nonmasterbookmark")
                .delete()
                .await?;

            sync_and_validate(&ctx, &commit_syncer, &mutable_counters).await?;
            Ok(())
        })
    }

    #[fbinit::test]
    fn test_simple_merge(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new()?;
        runtime.block_on_std(async move {
            let mutable_counters = SqlMutableCounters::with_sqlite_in_memory().unwrap();

            let ctx = CoreContext::test_mock(fb);
            let (syncers, _) = init_small_large_repo(&ctx).await?;
            let commit_syncer = syncers.small_to_large;
            let smallrepo = commit_syncer.get_source_repo();

            // Merge new repo
            let first_new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
                .add_file("firstnewrepo", "newcontent")
                .commit()
                .await?;
            let second_new_repo = CreateCommitContext::new(&ctx, &smallrepo, vec![first_new_repo])
                .add_file("secondnewrepo", "anothercontent")
                .commit()
                .await?;

            bookmark(&ctx, &smallrepo, "newrepohead")
                .set_to(second_new_repo)
                .await?;
            sync_and_validate(&ctx, &commit_syncer, &mutable_counters).await?;

            let merge = CreateCommitContext::new(&ctx, &smallrepo, vec!["master", "newrepohead"])
                .commit()
                .await?;

            bookmark(&ctx, &smallrepo, "master").set_to(merge).await?;
            sync_and_validate(&ctx, &commit_syncer, &mutable_counters).await?;

            // Diamond merges are not allowed
            let diamond_merge =
                CreateCommitContext::new(&ctx, &smallrepo, vec!["master", "newrepohead"])
                    .commit()
                    .await?;
            bookmark(&ctx, &smallrepo, "master")
                .set_to(diamond_merge)
                .await?;
            assert!(sync_and_validate(&ctx, &commit_syncer, &mutable_counters)
                .await
                .is_err());
            Ok(())
        })
    }

    #[fbinit::test]
    fn test_merge_added_in_single_bookmark_update(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new()?;
        runtime.block_on_std(async move {
            let mutable_counters = SqlMutableCounters::with_sqlite_in_memory().unwrap();

            let ctx = CoreContext::test_mock(fb);
            let (syncers, _) = init_small_large_repo(&ctx).await?;
            let commit_syncer = syncers.small_to_large;
            let smallrepo = commit_syncer.get_source_repo();

            // Merge new repo
            let first_new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
                .add_file("firstnewrepo", "newcontent")
                .commit()
                .await?;
            let second_new_repo = CreateCommitContext::new(&ctx, &smallrepo, vec![first_new_repo])
                .add_file("secondnewrepo", "anothercontent")
                .commit()
                .await?;

            let master_cs_id = resolve_cs_id(&ctx, &smallrepo, "master").await?;
            let merge =
                CreateCommitContext::new(&ctx, &smallrepo, vec![master_cs_id, second_new_repo])
                    .commit()
                    .await?;

            bookmark(&ctx, &smallrepo, "master").set_to(merge).await?;
            sync_and_validate(&ctx, &commit_syncer, &mutable_counters).await?;

            Ok(())
        })
    }

    #[fbinit::test]
    fn test_merge_of_a_merge_one_step(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new()?;
        runtime.block_on_std(async move {
            let mutable_counters = SqlMutableCounters::with_sqlite_in_memory().unwrap();

            let ctx = CoreContext::test_mock(fb);
            let (syncers, _) = init_small_large_repo(&ctx).await?;
            let commit_syncer = syncers.small_to_large;
            let smallrepo = commit_syncer.get_source_repo();

            // Merge new repo, which itself has a merge
            let first_new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
                .add_file("firstnewrepo", "newcontent")
                .commit()
                .await?;
            let second_new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
                .add_file("secondnewrepo", "anothercontent")
                .commit()
                .await?;

            let merge_new_repo =
                CreateCommitContext::new(&ctx, &smallrepo, vec![first_new_repo, second_new_repo])
                    .commit()
                    .await?;

            let master_cs_id = resolve_cs_id(&ctx, &smallrepo, "master").await?;
            let merge =
                CreateCommitContext::new(&ctx, &smallrepo, vec![master_cs_id, merge_new_repo])
                    .commit()
                    .await?;

            bookmark(&ctx, &smallrepo, "master").set_to(merge).await?;
            sync_and_validate(&ctx, &commit_syncer, &mutable_counters).await?;

            Ok(())
        })
    }

    #[fbinit::test]
    fn test_merge_of_a_merge_two_steps(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new()?;
        runtime.block_on_std(async move {
            let mutable_counters = SqlMutableCounters::with_sqlite_in_memory().unwrap();

            let ctx = CoreContext::test_mock(fb);
            let (syncers, _) = init_small_large_repo(&ctx).await?;
            let commit_syncer = syncers.small_to_large;
            let smallrepo = commit_syncer.get_source_repo();

            // Merge new repo, which itself has a merge
            let first_new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
                .add_file("firstnewrepo", "newcontent")
                .commit()
                .await?;
            let second_new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
                .add_file("secondnewrepo", "anothercontent")
                .commit()
                .await?;

            let merge_new_repo =
                CreateCommitContext::new(&ctx, &smallrepo, vec![first_new_repo, second_new_repo])
                    .commit()
                    .await?;
            bookmark(&ctx, &smallrepo, "newrepoimport")
                .set_to(merge_new_repo)
                .await?;
            sync_and_validate(&ctx, &commit_syncer, &mutable_counters).await?;

            let merge = CreateCommitContext::new(&ctx, &smallrepo, vec!["master", "newrepoimport"])
                .commit()
                .await?;

            bookmark(&ctx, &smallrepo, "master").set_to(merge).await?;
            sync_and_validate(&ctx, &commit_syncer, &mutable_counters).await?;

            Ok(())
        })
    }

    #[fbinit::test]
    fn test_merge_non_shared_bookmark(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new()?;
        runtime.block_on_std(async move {
            let mutable_counters = SqlMutableCounters::with_sqlite_in_memory().unwrap();

            let ctx = CoreContext::test_mock(fb);

            let (syncers, _) = init_small_large_repo(&ctx).await?;
            let commit_syncer = syncers.small_to_large;
            let smallrepo = commit_syncer.get_source_repo();

            let new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
                .add_file("firstnewrepo", "newcontent")
                .commit()
                .await?;
            bookmark(&ctx, &smallrepo, "newrepohead")
                .set_to(new_repo)
                .await?;
            sync_and_validate(&ctx, &commit_syncer, &mutable_counters).await?;

            let merge = CreateCommitContext::new(&ctx, &smallrepo, vec!["master", "newrepohead"])
                .commit()
                .await?;

            bookmark(&ctx, &smallrepo, "somebook").set_to(merge).await?;
            assert!(sync_and_validate(&ctx, &commit_syncer, &mutable_counters)
                .await
                .is_err());
            Ok(())
        })
    }

    async fn sync_and_validate(
        ctx: &CoreContext,
        commit_syncer: &CommitSyncer<SqlSyncedCommitMapping>,
        mutable_counters: &SqlMutableCounters,
    ) -> Result<(), Error> {
        let smallrepo = commit_syncer.get_source_repo();
        let megarepo = commit_syncer.get_target_repo();

        let counter = crate::format_counter(&commit_syncer);
        let target_repo_id = megarepo.get_repoid();
        let start_from = mutable_counters
            .get_counter(ctx.clone(), target_repo_id, &counter)
            .compat()
            .await?
            .unwrap_or(1);

        println!("start from: {}", start_from);
        let read_all = 65536;
        let log_entries = smallrepo
            .read_next_bookmark_log_entries(
                ctx.clone(),
                start_from as u64,
                read_all,
                Freshness::MostRecent,
            )
            .collect()
            .compat()
            .await?;

        println!(
            "syncing log entries {:?}  from repo#{} to repo#{}",
            log_entries,
            smallrepo.get_repoid(),
            megarepo.get_repoid()
        );

        let source_skiplist_index = Source(Arc::new(SkiplistIndex::new()));
        let target_skiplist_index = Target(Arc::new(SkiplistIndex::new()));
        let common_pushrebase_bookmarks = hashset! { BookmarkName::new("master")? };
        for entry in log_entries {
            let entry_id = entry.id;
            sync_single_bookmark_update_log(
                &ctx,
                &commit_syncer,
                entry,
                &source_skiplist_index.clone(),
                &target_skiplist_index.clone(),
                &common_pushrebase_bookmarks,
                ScubaSampleBuilder::with_discard(),
            )
            .await?;

            mutable_counters
                .set_counter(ctx.clone(), target_repo_id, &counter, entry_id, None)
                .compat()
                .await?;
        }

        assert_eq!(
            validation::find_bookmark_diff(ctx.clone(), commit_syncer).await?,
            vec![],
        );
        let heads = smallrepo
            .get_bonsai_heads_maybe_stale(ctx.clone())
            .collect()
            .compat()
            .await?;
        for head in heads {
            println!("verifying working copy for {}", head);
            validation::verify_working_copy(ctx.clone(), commit_syncer.clone(), head).await?;
        }

        Ok(())
    }
}
