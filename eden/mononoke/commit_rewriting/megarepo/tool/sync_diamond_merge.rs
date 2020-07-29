/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// This is a very hacky temporary tool that's used with only one purpose -
/// to half-manually sync a diamond merge commit from a small repo into a large repo.
/// NOTE - this is not a production quality tool, but rather a best effort attempt to
/// half-automate a rare case that might occur. Tool most likely doesn't cover all the cases.
/// USE WITH CARE!
use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobrepo_utils::convert_diff_result_into_file_change_for_diamond_merge;
use blobstore::Loadable;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use cloned::cloned;
use context::CoreContext;
use cross_repo_sync::{
    create_commit_syncers, rewrite_commit, update_mapping, upload_commits, CommitSyncOutcome,
    CommitSyncer, Syncers,
};
use futures::{
    compat::Future01CompatExt,
    future::TryFutureExt,
    stream::{futures_unordered::FuturesUnordered, TryStreamExt},
};
use futures_ext::{BoxStream, StreamExt};
use futures_old::{Future, IntoFuture, Stream};
use manifest::{bonsai_diff, BonsaiDiffFileChange};
use maplit::hashmap;
use mercurial_types::{HgFileNodeId, HgManifestId};
use metaconfig_types::RepoConfig;
use mononoke_types::{BonsaiChangeset, ChangesetId, FileChange, MPath};
use revset::DifferenceOfUnionsOfAncestorsNodeStream;
use skiplist::fetch_skiplist_index;
use slog::{info, warn};
use std::collections::{BTreeMap, HashMap};
use synced_commit_mapping::SqlSyncedCommitMapping;

/// The function syncs merge commit M from a small repo into a large repo.
/// It's designed to handle a case described below
///
///   Small repo state
///       M
///     |   \
///     P1  |   <- P1 must already be synced
///     |   |
///     |   P2 <- might not be synced yet
///    ...  |
///     |   /
///     |  /
///      ROOT
///
///   Large repo state
///
///     O   <- ONTO value (i.e. where onto_bookmark points to)
///    ...  <- commits from another small repo
///     |
///     P1' <- synced P1 commit from small repo
///     |
///     OVR' <- Potentially there can be commits from another repo between root and P1!
///      |
///     ROOT' <- synced ROOT commit
///
///
/// Most of the complexity stems from two facts
/// 1) If parents have different file content, then merge commit must have a file change entry for them
/// 2) that large repo might have rewritten commits from another small repo between ROOT' and P1'.
///
/// That means that rewritten M' bonsai object must contain file change entries that were changed
/// in OVR* commits.
///
/// So the function works as follows:
/// 1) Syncs all ROOT::P2 commits - nothing difficult here, just rewrite and save to large repo.
///    Those commits are expected to be non-merges for simplicity
/// 2) Create new merge commit
///    a) First find all the changed files from another small repo - those need to be in the merge repo
///       NOTE - we expect that all changes from this small repo are already in the bonsai changeset
///    b) Add file changes from previous step in the merge commit
///    c) Change parents
/// 3) Save merge commit in large repo
/// 4) Update the bookmark
pub async fn do_sync_diamond_merge(
    ctx: CoreContext,
    small_repo: BlobRepo,
    large_repo: BlobRepo,
    small_merge_cs_id: ChangesetId,
    mapping: SqlSyncedCommitMapping,
    small_repo_config: RepoConfig,
    onto_bookmark: BookmarkName,
) -> Result<(), Error> {
    info!(
        ctx.logger(),
        "Preparing to sync a merge commit {}...", small_merge_cs_id
    );

    let parents = small_repo
        .get_changeset_parents_by_bonsai(ctx.clone(), small_merge_cs_id)
        .compat()
        .await?;

    let (p1, p2) = validate_parents(parents)?;

    let new_branch =
        find_new_branch_oldest_first(ctx.clone(), &small_repo, p1, p2, &small_repo_config).await?;

    let syncers = create_commit_syncers(
        small_repo.clone(),
        large_repo.clone(),
        &small_repo_config
            .commit_sync_config
            .ok_or(Error::msg("Commit sync config is not specified"))?,
        mapping,
    )?;

    let small_root = find_root(&new_branch)?;

    info!(
        ctx.logger(),
        "{} new commits are going to be merged in",
        new_branch.len()
    );
    for bcs in new_branch {
        let cs_id = bcs.get_changeset_id();
        let parents = bcs.parents().collect::<Vec<_>>();
        if parents.len() > 1 {
            return Err(format_err!(
                "{} from branch contains more than one parent",
                cs_id
            ));
        }
        info!(ctx.logger(), "syncing commit from new branch {}", cs_id);
        syncers
            .small_to_large
            .unsafe_sync_commit(ctx.clone(), cs_id)
            .await?;
    }

    let maybe_onto_value = large_repo
        .get_bonsai_bookmark(ctx.clone(), &onto_bookmark)
        .compat()
        .await?;

    let onto_value =
        maybe_onto_value.ok_or(format_err!("cannot find bookmark {}", onto_bookmark))?;

    let rewritten = create_rewritten_merge_commit(
        ctx.clone(),
        small_merge_cs_id,
        &small_repo,
        &large_repo,
        &syncers,
        small_root,
        onto_value,
    )
    .await?;

    let new_merge_cs_id = rewritten.get_changeset_id();
    info!(ctx.logger(), "uploading merge commit {}", new_merge_cs_id);
    upload_commits(ctx.clone(), vec![rewritten], small_repo, large_repo.clone()).await?;

    update_mapping(
        ctx.clone(),
        hashmap! {small_merge_cs_id => new_merge_cs_id},
        &syncers.small_to_large,
    )
    .await?;

    let mut book_txn = large_repo.update_bookmark_transaction(ctx.clone());
    book_txn.force_set(
        &onto_bookmark,
        new_merge_cs_id,
        BookmarkUpdateReason::ManualMove,
        None,
    )?;
    book_txn.commit().await?;

    warn!(
        ctx.logger(),
        "It is recommended to run 'mononoke_admin crossrepo verify-wc' for {}!", new_merge_cs_id
    );
    Ok(())
}

async fn create_rewritten_merge_commit(
    ctx: CoreContext,
    small_merge_cs_id: ChangesetId,
    small_repo: &BlobRepo,
    large_repo: &BlobRepo,
    syncers: &Syncers<SqlSyncedCommitMapping>,
    small_root: ChangesetId,
    onto_value: ChangesetId,
) -> Result<BonsaiChangeset, Error> {
    let merge_bcs = small_merge_cs_id
        .load(ctx.clone(), small_repo.blobstore())
        .await?;

    let parents = merge_bcs.parents().collect();
    let (p1, p2) = validate_parents(parents)?;

    let merge_bcs = merge_bcs.into_mut();

    let large_root = remap_commit(ctx.clone(), &syncers.small_to_large, small_root).await?;
    let remapped_p2 = remap_commit(ctx.clone(), &syncers.small_to_large, p2).await?;

    let remapped_parents = hashmap! {
        p1 => onto_value,
        p2 => remapped_p2,
    };
    let maybe_rewritten = rewrite_commit(
        ctx.clone(),
        merge_bcs,
        &remapped_parents,
        syncers.small_to_large.get_mover().clone(),
        syncers.small_to_large.get_source_repo().clone(),
    )
    .await?;
    let mut rewritten =
        maybe_rewritten.ok_or(Error::msg("merge commit was unexpectedly rewritten out"))?;

    let mut additional_file_changes = generate_additional_file_changes(
        ctx.clone(),
        large_root,
        &large_repo,
        &syncers.large_to_small,
        onto_value,
    )
    .await?;

    for (path, fc) in rewritten.file_changes {
        additional_file_changes.insert(path, fc);
    }
    rewritten.file_changes = additional_file_changes;
    rewritten.freeze()
}

/// This function finds all the changed file between root and onto that are from another small repo.
/// These files needed to be added to the new merge commit to preserve bonsai semantic.
async fn generate_additional_file_changes(
    ctx: CoreContext,
    root: ChangesetId,
    large_repo: &BlobRepo,
    large_to_small: &CommitSyncer<SqlSyncedCommitMapping>,
    onto_value: ChangesetId,
) -> Result<BTreeMap<MPath, Option<FileChange>>, Error> {
    let bonsai_diff = find_bonsai_diff(ctx.clone(), &large_repo, root, onto_value)
        .collect()
        .compat()
        .await?;

    let additional_file_changes = FuturesUnordered::new();
    for diff_res in bonsai_diff {
        match diff_res {
            BonsaiDiffFileChange::Changed(ref path, ..)
            | BonsaiDiffFileChange::ChangedReusedId(ref path, ..)
            | BonsaiDiffFileChange::Deleted(ref path) => {
                let maybe_new_path = large_to_small.get_mover()(path)?;
                if maybe_new_path.is_some() {
                    continue;
                }
            }
        }

        let fc = convert_diff_result_into_file_change_for_diamond_merge(
            ctx.clone(),
            &large_repo,
            diff_res,
        )
        .compat();
        additional_file_changes.push(fc);
    }

    additional_file_changes
        .try_collect::<BTreeMap<_, _>>()
        .await
}

async fn remap_commit(
    ctx: CoreContext,
    small_to_large_commit_syncer: &CommitSyncer<SqlSyncedCommitMapping>,
    cs_id: ChangesetId,
) -> Result<ChangesetId, Error> {
    let maybe_sync_outcome = small_to_large_commit_syncer
        .get_commit_sync_outcome(ctx.clone(), cs_id)
        .await?;

    let sync_outcome = maybe_sync_outcome.ok_or(format_err!(
        "{} from small repo hasn't been remapped in large repo",
        cs_id
    ))?;

    use CommitSyncOutcome::*;
    match sync_outcome {
        RewrittenAs(ref cs_id, _) => Ok(*cs_id),
        Preserved => Ok(cs_id),
        _ => Err(format_err!(
            "unexpected commit sync outcome for root, got {:?}",
            sync_outcome
        )),
    }
}

fn find_root(new_branch: &Vec<BonsaiChangeset>) -> Result<ChangesetId, Error> {
    let mut cs_to_parents: HashMap<_, Vec<_>> = HashMap::new();
    for bcs in new_branch {
        let cs_id = bcs.get_changeset_id();
        cs_to_parents.insert(cs_id, bcs.parents().collect());
    }

    let mut roots = vec![];
    for parents in cs_to_parents.values() {
        for p in parents {
            if !cs_to_parents.contains_key(p) {
                roots.push(p);
            }
        }
    }

    validate_roots(roots).map(|root| *root)
}

async fn find_new_branch_oldest_first(
    ctx: CoreContext,
    small_repo: &BlobRepo,
    p1: ChangesetId,
    p2: ChangesetId,
    small_repo_config: &RepoConfig,
) -> Result<Vec<BonsaiChangeset>, Error> {
    let fetcher = small_repo.get_changeset_fetcher();
    let skiplist_index = fetch_skiplist_index(
        &ctx,
        &small_repo_config.skiplist_index_blobstore_key,
        &small_repo.get_blobstore().boxed(),
    )
    .await?;

    let new_branch = DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
        ctx.clone(),
        &fetcher,
        skiplist_index,
        vec![p2],
        vec![p1],
    )
    .map({
        cloned!(ctx, small_repo);
        move |cs| {
            cs.load(ctx.clone(), small_repo.blobstore())
                .compat()
                .from_err()
        }
    })
    .buffered(100)
    .collect()
    .compat()
    .await?;

    Ok(new_branch.into_iter().rev().collect())
}

fn validate_parents(parents: Vec<ChangesetId>) -> Result<(ChangesetId, ChangesetId), Error> {
    if parents.len() > 2 {
        return Err(format_err!(
            "too many parents, expected only 2: {:?}",
            parents
        ));
    }
    let p1 = parents.get(0).ok_or(Error::msg("not a merge commit"))?;
    let p2 = parents.get(1).ok_or(Error::msg("not a merge commit"))?;

    Ok((*p1, *p2))
}

fn validate_roots(roots: Vec<&ChangesetId>) -> Result<&ChangesetId, Error> {
    if roots.len() > 1 {
        return Err(format_err!("too many roots, expected only 1: {:?}", roots));
    }

    roots
        .get(0)
        .cloned()
        .ok_or(Error::msg("no roots found, this is not a diamond merge"))
}

fn find_bonsai_diff(
    ctx: CoreContext,
    repo: &BlobRepo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> BoxStream<BonsaiDiffFileChange<HgFileNodeId>, Error> {
    (
        id_to_manifestid(ctx.clone(), repo.clone(), descendant),
        id_to_manifestid(ctx.clone(), repo.clone(), ancestor),
    )
        .into_future()
        .map({
            cloned!(ctx, repo);
            move |(d_mf, a_mf)| {
                bonsai_diff(
                    ctx,
                    repo.get_blobstore(),
                    d_mf,
                    Some(a_mf).into_iter().collect(),
                )
            }
        })
        .flatten_stream()
        .boxify()
}

fn id_to_manifestid(
    ctx: CoreContext,
    repo: BlobRepo,
    bcs_id: ChangesetId,
) -> impl Future<Item = HgManifestId, Error = Error> {
    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
        .and_then({
            cloned!(ctx, repo);
            move |cs_id| {
                cs_id
                    .load(ctx.clone(), repo.blobstore())
                    .compat()
                    .from_err()
            }
        })
        .map(|cs| cs.manifestid())
}
