/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This is a very hacky temporary tool that's used with only one purpose -
//! to half-manually sync a diamond merge commit from a small repo into a large repo.
//! NOTE - this is not a production quality tool, but rather a best effort attempt to
//! half-automate a rare case that might occur. Tool most likely doesn't cover all the cases.
//! USE WITH CARE!

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::format_err;
use blobrepo_utils::convert_diff_result_into_file_change_for_diamond_merge;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use cloned::cloned;
use cmdlib_cross_repo::repo_provider_from_mononoke_app;
use commit_graph::CommitGraphRef;
use commit_transformation::upload_commits;
use context::CoreContext;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncData;
use cross_repo_sync::CommitSyncOutcome;
use cross_repo_sync::InMemoryRepo;
use cross_repo_sync::SubmoduleDeps;
use cross_repo_sync::SubmoduleExpansionData;
use cross_repo_sync::Syncers;
use cross_repo_sync::create_commit_syncers;
use cross_repo_sync::get_all_submodule_deps_from_repo_pair;
use cross_repo_sync::rewrite_commit;
use cross_repo_sync::submodule_metadata_file_prefix_and_dangling_pointers;
use cross_repo_sync::unsafe_sync_commit;
use cross_repo_sync::update_mapping_with_version;
use futures::future::try_join;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::stream::futures_unordered::FuturesUnordered;
use futures::try_join;
use live_commit_sync_config::LiveCommitSyncConfig;
use manifest::BonsaiDiffFileChange;
use manifest::bonsai_diff;
use maplit::hashmap;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::SourceRepoArgs;
use mononoke_app::args::TargetRepoArgs;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use slog::info;
use slog::warn;
use sorted_vector_map::SortedVectorMap;

use crate::commands::megarepo::common::get_live_commit_sync_config;

/// Sync a diamond merge commit from a small repo into large repo
#[derive(Debug, clap::Args)]
pub struct SyncDiamondMergeArgs {
    /// Diamond merge commit from small repo to sync
    #[clap(flatten)]
    pub merge_commit_hash: ChangesetArgs,

    /// Bookmark to point to resulting commits (no sanity checks, will move existing bookmark, be careful)
    #[clap(long)]
    pub onto_bookmark: Option<String>,

    #[clap(flatten)]
    source_repo: SourceRepoArgs,

    #[clap(flatten)]
    target_repo: TargetRepoArgs,
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: SyncDiamondMergeArgs) -> Result<()> {
    let target_repo_fut = app.open_repo(&args.target_repo);
    let source_repo_fut = app.open_repo(&args.source_repo);

    let (source_repo, target_repo): (Repo, Repo) =
        try_join(source_repo_fut, target_repo_fut).await?;

    let source_repo_id = source_repo.repo_identity().id();
    info!(
        ctx.logger(),
        "using repo \"{}\" repoid {:?}",
        source_repo.repo_identity().name(),
        source_repo_id
    );

    info!(
        ctx.logger(),
        "using repo \"{}\" repoid {:?}",
        target_repo.repo_identity().name(),
        target_repo.repo_identity().id()
    );
    let maybe_bookmark = args.onto_bookmark.map(BookmarkKey::new).transpose()?;

    let bookmark = maybe_bookmark.ok_or_else(|| Error::msg("bookmark must be specified"))?;

    let source_merge_cs_id = args
        .merge_commit_hash
        .resolve_changeset(ctx, &source_repo)
        .await?;
    info!(
        ctx.logger(),
        "changeset resolved as: {:?}", source_merge_cs_id
    );

    let repo_provider = repo_provider_from_mononoke_app(&app);

    let live_commit_sync_config = get_live_commit_sync_config(ctx, &app, &args.source_repo)
        .await
        .context("building live_commit_sync_config")?;

    let source_repo_arc = Arc::new(source_repo);
    let target_repo_arc = Arc::new(target_repo);
    let submodule_deps = get_all_submodule_deps_from_repo_pair(
        ctx,
        source_repo_arc.clone(),
        target_repo_arc.clone(),
        repo_provider,
    )
    .await?;

    do_sync_diamond_merge(
        ctx,
        source_repo_arc.as_ref(),
        target_repo_arc.as_ref(),
        submodule_deps,
        source_merge_cs_id,
        bookmark,
        live_commit_sync_config,
    )
    .await
    .map(|_| ())
}

/// The function syncs merge commit M from a small repo into a large repo.
/// It's designed to handle a case described below
///
/// ```text
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
/// ```
pub async fn do_sync_diamond_merge(
    ctx: &CoreContext,
    small_repo: &Repo,
    large_repo: &Repo,
    submodule_deps: SubmoduleDeps<Repo>,
    small_merge_cs_id: ChangesetId,
    onto_bookmark: BookmarkKey,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<(), Error> {
    info!(
        ctx.logger(),
        "Preparing to sync a merge commit {}...", small_merge_cs_id
    );

    let parents = small_repo
        .commit_graph()
        .changeset_parents(ctx, small_merge_cs_id)
        .await?;

    let (p1, p2) = validate_parents(parents.to_vec())?;

    let new_branch = find_new_branch_oldest_first(ctx.clone(), small_repo, p1, p2).await?;

    let syncers = create_commit_syncers(
        ctx,
        small_repo.clone(),
        large_repo.clone(),
        submodule_deps,
        live_commit_sync_config,
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
        // It is unclear if we can do something better than use an `Only`
        // hint here. Current thinking is: let the sync fail if one of
        // the `new_branch` commits rewrites into 2 commits in the target
        // repo. Manual remediation would be needed in that case.

        unsafe_sync_commit(
            ctx,
            cs_id,
            &syncers.small_to_large,
            CandidateSelectionHint::Only,
            CommitSyncContext::SyncDiamondMerge,
            None,
            false, // add_mapping_to_hg_extra
        )
        .await?;
    }

    let maybe_onto_value = large_repo
        .bookmarks()
        .get(
            ctx.clone(),
            &onto_bookmark,
            bookmarks::Freshness::MostRecent,
        )
        .await?;

    let onto_value =
        maybe_onto_value.ok_or_else(|| format_err!("cannot find bookmark {}", onto_bookmark))?;

    let (rewritten, version_for_merge) = create_rewritten_merge_commit(
        ctx.clone(),
        small_merge_cs_id,
        small_repo,
        large_repo,
        &syncers,
        small_root,
        onto_value,
    )
    .await?;

    let new_merge_cs_id = rewritten.get_changeset_id();
    info!(ctx.logger(), "uploading merge commit {}", new_merge_cs_id);
    let submodule_expansion_content_ids = Vec::<(Arc<Repo>, HashSet<_>)>::new();
    upload_commits(
        ctx,
        vec![rewritten],
        &small_repo,
        &large_repo,
        submodule_expansion_content_ids,
    )
    .await?;

    update_mapping_with_version(
        ctx,
        hashmap! {small_merge_cs_id => new_merge_cs_id},
        &syncers.small_to_large,
        &version_for_merge,
    )
    .await?;

    let mut book_txn = large_repo.bookmarks().create_transaction(ctx.clone());
    book_txn.force_set(
        &onto_bookmark,
        new_merge_cs_id,
        BookmarkUpdateReason::ManualMove,
    )?;
    book_txn.commit().await?;

    warn!(
        ctx.logger(),
        "It is recommended to run 'mononoke_admin cross-repo verify-working-copy' for {}!",
        new_merge_cs_id
    );
    Ok(())
}

async fn create_rewritten_merge_commit(
    ctx: CoreContext,
    small_merge_cs_id: ChangesetId,
    small_repo: &Repo,
    large_repo: &Repo,
    syncers: &Syncers<Repo>,
    small_root: ChangesetId,
    onto_value: ChangesetId,
) -> Result<(BonsaiChangeset, CommitSyncConfigVersion), Error> {
    let merge_bcs = small_merge_cs_id
        .load(&ctx, small_repo.repo_blobstore())
        .await?;

    let parents = merge_bcs.parents().collect();
    let (p1, p2) = validate_parents(parents)?;

    let merge_bcs = merge_bcs.into_mut();

    // For simplicity sake allow doing the diamond merge only if there were
    // no changes in the mapping versions.
    let (large_root, root_version) = remap_commit(ctx.clone(), &syncers.small_to_large, small_root)
        .await
        .context("error remapping small root commit")?;

    let (_, version_p1) = remap_commit(ctx.clone(), &syncers.small_to_large, p1)
        .await
        .context("error remapping small p1 commit")?;
    let (remapped_p2, version_p2) = remap_commit(ctx.clone(), &syncers.small_to_large, p2)
        .await
        .context("error remapping small p2 commit")?;

    if version_p1 != version_p2 {
        return Err(format_err!(
            "Parents are remapped with different commit sync config versions: {} vs {}",
            version_p1,
            version_p2
        ));
    }

    if root_version != version_p1 {
        return Err(format_err!(
            "Commit sync version of root commit is different from p1 version: {} vs {}",
            root_version,
            version_p1,
        ));
    }

    let remapped_parents = hashmap! {
        p1 => onto_value,
        p2 => remapped_p2,
    };

    let (x_repo_submodule_metadata_file_prefix, dangling_submodule_pointers) =
        submodule_metadata_file_prefix_and_dangling_pointers(
            small_repo.repo_identity().id(),
            &root_version,
            syncers.small_to_large.live_commit_sync_config.clone(),
        )
        .await?;

    let submodule_deps = syncers.small_to_large.get_submodule_deps();

    let small_repo_id = small_repo.repo_identity().id();
    let fallback_repos = vec![Arc::new(small_repo.clone())]
        .into_iter()
        .chain(submodule_deps.repos())
        .collect::<Vec<_>>();
    let large_in_memory_repo = InMemoryRepo::from_repo(large_repo, fallback_repos)?;
    let submodule_expansion_data = match submodule_deps {
        SubmoduleDeps::ForSync(deps) => Some(SubmoduleExpansionData {
            submodule_deps: deps,
            x_repo_submodule_metadata_file_prefix: x_repo_submodule_metadata_file_prefix.as_str(),
            small_repo_id,
            large_repo: large_in_memory_repo,
            dangling_submodule_pointers,
        }),
        SubmoduleDeps::NotNeeded | SubmoduleDeps::NotAvailable => None,
    };

    let source_repo = syncers.small_to_large.get_source_repo();
    let rewrite_res = rewrite_commit(
        &ctx,
        merge_bcs,
        &remapped_parents,
        syncers
            .small_to_large
            .get_movers_by_version(&version_p1)
            .await?,
        source_repo,
        Default::default(),
        Default::default(),
        submodule_expansion_data,
    )
    .await?;
    let mut rewritten = rewrite_res
        .rewritten
        .ok_or_else(|| Error::msg("merge commit was unexpectedly rewritten out"))?;

    let mut additional_file_changes = generate_additional_file_changes(
        ctx.clone(),
        large_root,
        large_repo,
        &syncers.large_to_small,
        onto_value,
        &root_version,
    )
    .await?;

    for (path, fc) in rewritten.file_changes {
        additional_file_changes.insert(path, fc);
    }
    rewritten.file_changes = additional_file_changes;
    let cs_id = rewritten.freeze()?;
    Ok((cs_id, version_p1))
}

/// This function finds all the changed file between root and onto that are from another small repo.
/// These files needed to be added to the new merge commit to preserve bonsai semantic.
async fn generate_additional_file_changes(
    ctx: CoreContext,
    root: ChangesetId,
    large_repo: &Repo,
    large_to_small: &CommitSyncData<Repo>,
    onto_value: ChangesetId,
    version: &CommitSyncConfigVersion,
) -> Result<SortedVectorMap<NonRootMPath, FileChange>, Error> {
    let bonsai_diff = find_bonsai_diff(ctx.clone(), large_repo, root, onto_value)
        .try_collect::<Vec<_>>()
        .await?;

    let additional_file_changes = FuturesUnordered::new();
    for diff_res in bonsai_diff {
        let mover = large_to_small.get_movers_by_version(version).await?.mover;
        let maybe_new_path = mover.move_path(diff_res.path())?;
        if maybe_new_path.is_some() {
            continue;
        }

        let fc = convert_diff_result_into_file_change_for_diamond_merge(&ctx, large_repo, diff_res);
        additional_file_changes.push(fc);
    }

    additional_file_changes
        .try_collect::<SortedVectorMap<_, _>>()
        .await
}

async fn remap_commit(
    ctx: CoreContext,
    small_to_large_commit_syncer: &CommitSyncData<Repo>,
    cs_id: ChangesetId,
) -> Result<(ChangesetId, CommitSyncConfigVersion), Error> {
    let maybe_sync_outcome = small_to_large_commit_syncer
        .get_commit_sync_outcome(&ctx, cs_id)
        .await?;

    let sync_outcome = maybe_sync_outcome.ok_or_else(|| {
        format_err!(
            "{} from small repo hasn't been remapped in large repo",
            cs_id
        )
    })?;

    use CommitSyncOutcome::*;
    match sync_outcome {
        RewrittenAs(ref cs_id, ref version) => Ok((*cs_id, version.clone())),
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

    validate_roots(roots).copied()
}

async fn find_new_branch_oldest_first(
    ctx: CoreContext,
    small_repo: &Repo,
    p1: ChangesetId,
    p2: ChangesetId,
) -> Result<Vec<BonsaiChangeset>, Error> {
    let new_branch = small_repo
        .commit_graph()
        .ancestors_difference_stream(&ctx, vec![p2], vec![p1])
        .await?
        .map_ok({
            cloned!(ctx, small_repo);
            move |cs| {
                cloned!(ctx, small_repo);
                async move { Ok(cs.load(&ctx, small_repo.repo_blobstore()).await?) }
            }
        })
        .try_buffered(100)
        .try_collect::<Vec<_>>()
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
    let p1 = parents
        .first()
        .ok_or_else(|| Error::msg("not a merge commit"))?;
    let p2 = parents
        .get(1)
        .ok_or_else(|| Error::msg("not a merge commit"))?;

    Ok((*p1, *p2))
}

fn validate_roots(roots: Vec<&ChangesetId>) -> Result<&ChangesetId, Error> {
    if roots.len() > 1 {
        return Err(format_err!("too many roots, expected only 1: {:?}", roots));
    }

    roots
        .first()
        .cloned()
        .ok_or_else(|| Error::msg("no roots found, this is not a diamond merge"))
}

fn find_bonsai_diff(
    ctx: CoreContext,
    repo: &Repo,
    ancestor: ChangesetId,
    descendant: ChangesetId,
) -> BoxStream<'static, Result<BonsaiDiffFileChange<(FileType, HgFileNodeId)>, Error>> {
    stream::once({
        cloned!(ctx, repo);
        async move {
            try_join!(
                id_to_manifestid(ctx.clone(), repo.clone(), descendant),
                id_to_manifestid(ctx, repo, ancestor)
            )
        }
    })
    .map_ok({
        cloned!(ctx, repo);
        move |(d_mf, a_mf)| {
            bonsai_diff(
                ctx.clone(),
                repo.repo_blobstore().clone(),
                d_mf,
                Some(a_mf).into_iter().collect(),
            )
            .boxed()
        }
    })
    .try_flatten()
    .boxed()
}

async fn id_to_manifestid(
    ctx: CoreContext,
    repo: Repo,
    bcs_id: ChangesetId,
) -> Result<HgManifestId, Error> {
    let cs_id = repo.derive_hg_changeset(&ctx, bcs_id).await?;
    let cs = cs_id.load(&ctx, repo.repo_blobstore()).await?;
    Ok(cs.manifestid())
}
