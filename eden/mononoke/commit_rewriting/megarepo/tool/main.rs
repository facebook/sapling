/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::bail;
use anyhow::format_err;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLog;
use bookmarks::Bookmarks;
use clap::ArgMatches;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use cmdlib_x_repo::create_commit_syncer_from_matches;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncOutcome;
use cross_repo_sync::CommitSyncer;
use cross_repo_sync::ConcreteRepo as CrossRepo;
use cross_repo_sync::find_toposorted_unsynced_ancestors;
use cross_repo_sync::unsafe_sync_commit;
use fbinit::FacebookInit;
use filenodes::Filenodes;
use filestore::FilestoreConfig;
use fsnodes::RootFsnodeId;
use futures::TryStreamExt;
use futures::future::try_join;
use futures::future::try_join_all;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathOrPrefix;
use megarepolib::common::create_and_save_bonsai;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::RepoConfig;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::path::MPath;
use movers::Mover;
use mutable_counters::MutableCounters;
use phases::Phases;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use regex::Regex;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use slog::info;
use sql_query_config::SqlQueryConfig;

use crate::cli::CATCHUP_DELETE_HEAD;
use crate::cli::CATCHUP_VALIDATE_COMMAND;
use crate::cli::CHANGESET;
use crate::cli::COMMIT_BOOKMARK;
use crate::cli::COMMIT_HASH;
use crate::cli::DELETE_NO_LONGER_BOUND_FILES_FROM_LARGE_REPO;
use crate::cli::DELETION_CHUNK_SIZE;
use crate::cli::DRY_RUN;
use crate::cli::GRADUAL_MERGE;
use crate::cli::GRADUAL_MERGE_PROGRESS;
use crate::cli::HEAD_BOOKMARK;
use crate::cli::LAST_DELETION_COMMIT;
use crate::cli::LIMIT;
use crate::cli::MANUAL_COMMIT_SYNC;
use crate::cli::MAPPING_VERSION_NAME;
use crate::cli::PARENTS;
use crate::cli::PATH_PREFIX;
use crate::cli::PATH_REGEX;
use crate::cli::PRE_DELETION_COMMIT;
use crate::cli::SELECT_PARENTS_AUTOMATICALLY;
use crate::cli::SYNC_COMMIT_AND_ANCESTORS;
use crate::cli::TO_MERGE_CS_ID;
use crate::cli::WAIT_SECS;
use crate::cli::cs_args_from_matches;
use crate::cli::get_catchup_head_delete_commits_cs_args_factory;
use crate::cli::get_gradual_merge_commits_cs_args_factory;
use crate::cli::setup_app;

mod catchup;
mod cli;
mod gradual_merge;
mod manual_commit_sync;

#[derive(Clone)]
#[facet::container]
pub struct Repo(
    dyn BonsaiHgMapping,
    dyn BonsaiGitMapping,
    dyn BonsaiGlobalrevMapping,
    dyn PushrebaseMutationMapping,
    RepoCrossRepo,
    RepoBookmarkAttrs,
    dyn Bookmarks,
    dyn Phases,
    dyn BookmarkUpdateLog,
    FilestoreConfig,
    dyn MutableCounters,
    RepoBlobstore,
    RepoConfig,
    RepoDerivedData,
    RepoIdentity,
    CommitGraph,
    dyn CommitGraphWriter,
    dyn Filenodes,
    SqlQueryConfig,
);

async fn run_gradual_merge<'a>(
    ctx: &CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let config_store = matches.config_store();
    let repo: Repo =
        args::not_shardmanager_compatible::open_repo(ctx.fb, ctx.logger(), matches).await?;

    let last_deletion_commit = sub_m
        .value_of(LAST_DELETION_COMMIT)
        .ok_or_else(|| format_err!("last deletion commit is not specified"))?;
    let pre_deletion_commit = sub_m
        .value_of(PRE_DELETION_COMMIT)
        .ok_or_else(|| format_err!("pre deletion commit is not specified"))?;
    let bookmark = sub_m
        .value_of(COMMIT_BOOKMARK)
        .ok_or_else(|| format_err!("bookmark where to merge is not specified"))?;
    let dry_run = sub_m.is_present(DRY_RUN);

    let limit = args::get_usize_opt(sub_m, LIMIT);
    let (_, repo_config) =
        args::get_config_by_repoid(config_store, matches, repo.repo_identity().id())?;
    let last_deletion_commit = helpers::csid_resolve(ctx, &repo, last_deletion_commit);
    let pre_deletion_commit = helpers::csid_resolve(ctx, &repo, pre_deletion_commit);

    let (last_deletion_commit, pre_deletion_commit) =
        try_join(last_deletion_commit, pre_deletion_commit).await?;

    let merge_changeset_args_factory = get_gradual_merge_commits_cs_args_factory(sub_m)?;
    let params = gradual_merge::GradualMergeParams {
        pre_deletion_commit,
        last_deletion_commit,
        bookmark_to_merge_into: BookmarkKey::new(bookmark)?,
        merge_changeset_args_factory,
        limit,
        dry_run,
    };
    gradual_merge::gradual_merge(ctx, &repo, &params, &repo_config.pushrebase.flags).await?;

    Ok(())
}

async fn run_gradual_merge_progress<'a>(
    ctx: &CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo: Repo =
        args::not_shardmanager_compatible::open_repo(ctx.fb, ctx.logger(), matches).await?;

    let last_deletion_commit = sub_m
        .value_of(LAST_DELETION_COMMIT)
        .ok_or_else(|| format_err!("last deletion commit is not specified"))?;
    let pre_deletion_commit = sub_m
        .value_of(PRE_DELETION_COMMIT)
        .ok_or_else(|| format_err!("pre deletion commit is not specified"))?;
    let bookmark = sub_m
        .value_of(COMMIT_BOOKMARK)
        .ok_or_else(|| format_err!("bookmark where to merge is not specified"))?;

    let last_deletion_commit = helpers::csid_resolve(ctx, &repo, last_deletion_commit);
    let pre_deletion_commit = helpers::csid_resolve(ctx, &repo, pre_deletion_commit);

    let (last_deletion_commit, pre_deletion_commit) =
        try_join(last_deletion_commit, pre_deletion_commit).await?;

    let (done, total) = gradual_merge::gradual_merge_progress(
        ctx,
        &repo,
        &pre_deletion_commit,
        &last_deletion_commit,
        &BookmarkKey::new(bookmark)?,
    )
    .await?;

    println!("{}/{}", done, total);

    Ok(())
}

async fn run_manual_commit_sync<'a>(
    ctx: &CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer = create_commit_syncer_from_matches::<Repo>(ctx, matches, None).await?;

    let target_repo = commit_syncer.get_target_repo();
    let target_repo_parents = if sub_m.is_present(SELECT_PARENTS_AUTOMATICALLY) {
        None
    } else {
        let target_repo_parents = sub_m.values_of(PARENTS);
        match target_repo_parents {
            Some(target_repo_parents) => Some(
                try_join_all(
                    target_repo_parents
                        .into_iter()
                        .map(|p| helpers::csid_resolve(ctx, target_repo, p)),
                )
                .await?,
            ),
            None => Some(vec![]),
        }
    };

    let source_cs = sub_m
        .value_of(CHANGESET)
        .ok_or_else(|| format_err!("{} not set", CHANGESET))?;
    let source_repo = commit_syncer.get_source_repo();
    let source_cs_id = helpers::csid_resolve(ctx, source_repo, source_cs).await?;

    let mapping_version_name = sub_m
        .value_of(MAPPING_VERSION_NAME)
        .ok_or_else(|| format_err!("mapping-version-name is not specified"))?;

    let target_cs_id = manual_commit_sync::manual_commit_sync(
        ctx,
        &commit_syncer,
        source_cs_id,
        target_repo_parents,
        CommitSyncConfigVersion(mapping_version_name.to_string()),
    )
    .await?;
    info!(ctx.logger(), "target cs id is {:?}", target_cs_id);
    Ok(())
}

async fn run_catchup_delete_head<'a>(
    ctx: &CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo: Repo =
        args::not_shardmanager_compatible::open_repo(ctx.fb, &ctx.logger().clone(), matches)
            .await?;

    let head_bookmark = sub_m
        .value_of(HEAD_BOOKMARK)
        .ok_or_else(|| format_err!("{} not set", HEAD_BOOKMARK))?;

    let head_bookmark = BookmarkKey::new(head_bookmark)?;

    let to_merge_cs_id = sub_m
        .value_of(TO_MERGE_CS_ID)
        .ok_or_else(|| format_err!("{} not set", TO_MERGE_CS_ID))?;
    let to_merge_cs_id = helpers::csid_resolve(ctx, &repo, to_merge_cs_id).await?;

    let path_regex = sub_m
        .value_of(PATH_REGEX)
        .ok_or_else(|| format_err!("{} not set", PATH_REGEX))?;
    let path_regex = Regex::new(path_regex)?;

    let deletion_chunk_size = args::get_usize(&sub_m, DELETION_CHUNK_SIZE, 10000);

    let config_store = matches.config_store();
    let cs_args_factory = get_catchup_head_delete_commits_cs_args_factory(sub_m)?;
    let (_, repo_config) = args::not_shardmanager_compatible::get_config(config_store, matches)?;

    let wait_secs = args::get_u64(&sub_m, WAIT_SECS, 0);

    catchup::create_deletion_head_commits(
        ctx,
        &repo,
        head_bookmark,
        to_merge_cs_id,
        path_regex,
        deletion_chunk_size,
        cs_args_factory,
        &repo_config.pushrebase.flags,
        wait_secs,
    )
    .await?;
    Ok(())
}

async fn run_catchup_validate<'a>(
    ctx: &CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo: Repo =
        args::not_shardmanager_compatible::open_repo(ctx.fb, &ctx.logger().clone(), matches)
            .await?;

    let result_commit = sub_m
        .value_of(COMMIT_HASH)
        .ok_or_else(|| format_err!("{} not set", COMMIT_HASH))?;
    let to_merge_cs_id = sub_m
        .value_of(TO_MERGE_CS_ID)
        .ok_or_else(|| format_err!("{} not set", TO_MERGE_CS_ID))?;
    let result_commit = helpers::csid_resolve(ctx, &repo, result_commit);

    let to_merge_cs_id = helpers::csid_resolve(ctx, &repo, to_merge_cs_id);

    let (result_commit, to_merge_cs_id) = try_join(result_commit, to_merge_cs_id).await?;

    let path_regex = sub_m
        .value_of(PATH_REGEX)
        .ok_or_else(|| format_err!("{} not set", PATH_REGEX))?;
    let path_regex = Regex::new(path_regex)?;

    catchup::validate(ctx, &repo, result_commit, to_merge_cs_id, path_regex).await?;

    Ok(())
}

async fn run_sync_commit_and_ancestors<'a>(
    ctx: &CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer =
        create_commit_syncer_from_matches::<cross_repo_sync::ConcreteRepo>(ctx, matches, None)
            .await?;

    let source_commit_hash = sub_m
        .value_of(COMMIT_HASH)
        .ok_or_else(|| format_err!("{} not specified", COMMIT_HASH))?;

    let source_cs_id =
        helpers::csid_resolve(ctx, commit_syncer.get_source_repo(), source_commit_hash).await?;

    let (unsynced_ancestors, _) =
        find_toposorted_unsynced_ancestors(ctx, &commit_syncer, source_cs_id, None).await?;

    for ancestor in unsynced_ancestors {
        unsafe_sync_commit(
            ctx,
            ancestor,
            &commit_syncer,
            CandidateSelectionHint::Only,
            CommitSyncContext::AdminChangeMapping,
            None,
            false, // add_mapping_to_hg_extra
        )
        .await?;
    }

    let commit_sync_outcome = commit_syncer
        .get_commit_sync_outcome(ctx, source_cs_id)
        .await?
        .ok_or_else(|| format_err!("was not able to remap a commit {}", source_cs_id))?;
    info!(ctx.logger(), "remapped to {:?}", commit_sync_outcome);

    Ok(())
}

async fn run_delete_no_longer_bound_files_from_large_repo<'a>(
    ctx: &CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer = create_commit_syncer_from_matches::<CrossRepo>(ctx, matches, None).await?;
    let large_repo = commit_syncer.get_large_repo();
    if commit_syncer.get_source_repo().repo_identity().id() != large_repo.repo_identity().id() {
        return Err(format_err!("source repo must be large!"));
    }

    let cs_id = sub_m
        .value_of(COMMIT_HASH)
        .ok_or_else(|| format_err!("{} not specified", COMMIT_HASH))?;
    let cs_id = helpers::csid_resolve(ctx, commit_syncer.get_source_repo(), cs_id).await?;

    // Find all files under a given path
    let prefix = sub_m.value_of(PATH_PREFIX).context("prefix is not set")?;
    let root_fsnode_id = large_repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, cs_id)
        .await?;
    let entries = root_fsnode_id
        .fsnode_id()
        .find_entries(
            ctx.clone(),
            large_repo.repo_blobstore().clone(),
            vec![PathOrPrefix::Prefix(MPath::new(prefix)?)],
        )
        .try_collect::<Vec<_>>()
        .await?;

    // Now find which files does not remap to a small repo - these files we want to delete
    let mover = find_mover_for_commit(ctx, &commit_syncer, cs_id).await?;

    let mut to_delete = vec![];
    for (path, entry) in entries {
        if let Entry::Leaf(_) = entry {
            let path = path.try_into().unwrap();
            if mover.move_path(&path)?.is_none() {
                to_delete.push(path);
            }
        }
    }

    if to_delete.is_empty() {
        info!(ctx.logger(), "nothing to delete, exiting");
        return Ok(());
    }
    info!(ctx.logger(), "need to delete {} paths", to_delete.len());

    let resulting_changeset_args = cs_args_from_matches(sub_m)?;
    let deletion_cs_id = create_and_save_bonsai(
        ctx,
        large_repo,
        vec![cs_id],
        to_delete
            .into_iter()
            .map(|file| (file, FileChange::Deletion))
            .collect(),
        resulting_changeset_args,
    )
    .await?;

    info!(ctx.logger(), "created changeset {}", deletion_cs_id);

    Ok(())
}

async fn find_mover_for_commit<R: cross_repo_sync::Repo>(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncer<R>,
    cs_id: ChangesetId,
) -> Result<Arc<dyn Mover>, Error> {
    let maybe_sync_outcome = commit_syncer.get_commit_sync_outcome(ctx, cs_id).await?;

    let sync_outcome = maybe_sync_outcome.context("source commit was not remapped yet")?;
    use CommitSyncOutcome::*;
    let mover = match sync_outcome {
        NotSyncCandidate(_) => {
            return Err(format_err!(
                "commit is a not sync candidate, can't get a mover for this commit"
            ));
        }
        RewrittenAs(_, version) | EquivalentWorkingCopyAncestor(_, version) => {
            commit_syncer.get_movers_by_version(&version).await?.mover
        }
    };

    Ok(mover)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = setup_app();
    let (matches, _runtime) = app.get_matches(fb)?;
    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger_and_client_info(
        fb,
        logger.clone(),
        ClientInfo::default_with_entry_point(ClientEntryPoint::MegarepoTool),
    );
    let ctx = &ctx;

    let subcommand_future = async {
        match matches.subcommand() {
            (MANUAL_COMMIT_SYNC, Some(sub_m)) => run_manual_commit_sync(ctx, &matches, sub_m).await,
            (SYNC_COMMIT_AND_ANCESTORS, Some(sub_m)) => {
                run_sync_commit_and_ancestors(ctx, &matches, sub_m).await
            }

            // All commands relevant to gradual merge
            (CATCHUP_DELETE_HEAD, Some(sub_m)) => {
                run_catchup_delete_head(ctx, &matches, sub_m).await
            }
            (CATCHUP_VALIDATE_COMMAND, Some(sub_m)) => {
                run_catchup_validate(ctx, &matches, sub_m).await
            }
            (GRADUAL_MERGE, Some(sub_m)) => run_gradual_merge(ctx, &matches, sub_m).await,
            (GRADUAL_MERGE_PROGRESS, Some(sub_m)) => {
                run_gradual_merge_progress(ctx, &matches, sub_m).await
            }
            (DELETE_NO_LONGER_BOUND_FILES_FROM_LARGE_REPO, Some(sub_m)) => {
                run_delete_no_longer_bound_files_from_large_repo(ctx, &matches, sub_m).await
            }
            _ => bail!("oh no, wrong arguments provided!"),
        }
    };

    helpers::block_execute(
        subcommand_future,
        fb,
        "megarepotool",
        logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
