/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use borrowed::borrowed;
use clap::ArgMatches;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use cmdlib_x_repo::create_commit_syncer_from_matches;
use context::CoreContext;
use cross_repo_sync::create_commit_syncer_lease;
use cross_repo_sync::find_toposorted_unsynced_ancestors;
use cross_repo_sync::types::Source;
use cross_repo_sync::types::Target;
use cross_repo_sync::validation::verify_working_copy_with_version_fast_path;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncOutcome;
use cross_repo_sync::CommitSyncer;
use derived_data::BonsaiDerived;
use fbinit::FacebookInit;
use fsnodes::RootFsnodeId;
use futures::compat::Future01CompatExt;
use futures::future::try_join;
use futures::future::try_join_all;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use live_commit_sync_config::LiveCommitSyncConfig;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathOrPrefix;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::MetadataDatabaseConfig;
use mononoke_api_types::InnerRepo;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::MPath;
use mononoke_types::RepositoryId;
use movers::get_small_to_large_mover;
use movers::Mover;
use regex::Regex;
use slog::info;
use slog::warn;
#[cfg(fbcode_build)]
use sql_ext::facebook::MyAdmin;
use sql_ext::replication::NoReplicaLagMonitor;
use sql_ext::replication::ReplicaLagMonitor;
use sql_ext::replication::WaitForReplicationConfig;
use std::num::NonZeroU64;
use std::sync::Arc;
use synced_commit_mapping::EquivalentWorkingCopyEntry;
use synced_commit_mapping::SqlSyncedCommitMapping;
use synced_commit_mapping::SyncedCommitMapping;
use synced_commit_mapping::SyncedCommitMappingEntry;
use synced_commit_mapping::WorkingCopyEquivalence;
use tokio::fs::read_to_string;
use tokio::fs::File;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;

mod catchup;
mod cli;
mod gradual_merge;
mod manual_commit_sync;
mod merging;
mod sync_diamond_merge;

use crate::cli::cs_args_from_matches;
use crate::cli::get_catchup_head_delete_commits_cs_args_factory;
use crate::cli::get_delete_commits_cs_args_factory;
use crate::cli::get_gradual_merge_commits_cs_args_factory;
use crate::cli::setup_app;
use crate::cli::BACKFILL_NOOP_MAPPING;
use crate::cli::BASE_COMMIT_HASH;
use crate::cli::BONSAI_MERGE;
use crate::cli::BONSAI_MERGE_P1;
use crate::cli::BONSAI_MERGE_P2;
use crate::cli::CATCHUP_DELETE_HEAD;
use crate::cli::CATCHUP_VALIDATE_COMMAND;
use crate::cli::CHANGESET;
use crate::cli::CHECK_PUSH_REDIRECTION_PREREQS;
use crate::cli::CHUNKING_HINT_FILE;
use crate::cli::COMMIT_BOOKMARK;
use crate::cli::COMMIT_HASH;
use crate::cli::COMMIT_HASH_CORRECT_HISTORY;
use crate::cli::DELETE_NO_LONGER_BOUND_FILES_FROM_LARGE_REPO;
use crate::cli::DELETION_CHUNK_SIZE;
use crate::cli::DIFF_MAPPING_VERSIONS;
use crate::cli::DRY_RUN;
use crate::cli::EVEN_CHUNK_SIZE;
use crate::cli::FIRST_PARENT;
use crate::cli::GRADUAL_DELETE;
use crate::cli::GRADUAL_MERGE;
use crate::cli::GRADUAL_MERGE_PROGRESS;
use crate::cli::HEAD_BOOKMARK;
use crate::cli::HISTORY_FIXUP_DELETE;
use crate::cli::INPUT_FILE;
use crate::cli::LAST_DELETION_COMMIT;
use crate::cli::LIMIT;
use crate::cli::MANUAL_COMMIT_SYNC;
use crate::cli::MAPPING_VERSION_NAME;
use crate::cli::MARK_NOT_SYNCED_COMMAND;
use crate::cli::MAX_NUM_OF_MOVES_IN_COMMIT;
use crate::cli::MERGE;
use crate::cli::MOVE;
use crate::cli::ORIGIN_REPO;
use crate::cli::OVERWRITE;
use crate::cli::PARENTS;
use crate::cli::PATH;
use crate::cli::PATHS_FILE;
use crate::cli::PATH_PREFIX;
use crate::cli::PATH_REGEX;
use crate::cli::PRE_DELETION_COMMIT;
use crate::cli::PRE_MERGE_DELETE;
use crate::cli::RUN_MOVER;
use crate::cli::SECOND_PARENT;
use crate::cli::SELECT_PARENTS_AUTOMATICALLY;
use crate::cli::SOURCE_CHANGESET;
use crate::cli::SYNC_COMMIT_AND_ANCESTORS;
use crate::cli::SYNC_DIAMOND_MERGE;
use crate::cli::TARGET_CHANGESET;
use crate::cli::TO_MERGE_CS_ID;
use crate::cli::VERSION;
use crate::cli::WAIT_SECS;
use crate::merging::perform_merge;
use megarepolib::chunking::even_chunker_with_max_size;
use megarepolib::chunking::parse_chunking_hint;
use megarepolib::chunking::path_chunker_from_hint;
use megarepolib::chunking::Chunker;
use megarepolib::commit_sync_config_utils::diff_small_repo_commit_sync_configs;
use megarepolib::common::create_and_save_bonsai;
use megarepolib::common::delete_files_in_chunks;
use megarepolib::common::StackPosition;
use megarepolib::history_fixup_delete::create_history_fixup_deletes;
use megarepolib::history_fixup_delete::HistoryFixupDeletes;
use megarepolib::perform_move;
use megarepolib::perform_stack_move;
use megarepolib::pre_merge_delete::create_pre_merge_delete;
use megarepolib::pre_merge_delete::PreMergeDelete;
use megarepolib::working_copy::get_working_copy_paths_by_prefixes;

async fn run_move<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
    live_commit_sync_config: CfgrLiveCommitSyncConfig,
) -> Result<(), Error> {
    let origin_repo =
        RepositoryId::new(args::get_i32_opt(sub_m, ORIGIN_REPO).expect("Origin repo is missing"));
    let resulting_changeset_args = cs_args_from_matches(sub_m);
    let move_parent = sub_m.value_of(CHANGESET).unwrap().to_owned();

    let mapping_version_name = sub_m
        .value_of(MAPPING_VERSION_NAME)
        .ok_or_else(|| format_err!("mapping-version-name is not specified"))?;
    let mapping_version = CommitSyncConfigVersion(mapping_version_name.to_string());

    let commit_sync_config = live_commit_sync_config
        .get_commit_sync_config_by_version(origin_repo, &mapping_version)
        .await?;
    let mover = get_small_to_large_mover(&commit_sync_config, origin_repo).unwrap();

    let max_num_of_moves_in_commit: Option<NonZeroU64> =
        args::get_and_parse_opt(sub_m, MAX_NUM_OF_MOVES_IN_COMMIT);

    let (repo, resulting_changeset_args) = try_join(
        args::open_repo::<BlobRepo>(ctx.fb, &ctx.logger().clone(), matches),
        resulting_changeset_args.compat(),
    )
    .await?;

    let parent_bcs_id = helpers::csid_resolve(&ctx, &repo, move_parent).await?;

    if let Some(max_num_of_moves_in_commit) = max_num_of_moves_in_commit {
        let changesets = perform_stack_move(
            &ctx,
            &repo,
            parent_bcs_id,
            mover,
            max_num_of_moves_in_commit,
            |num: StackPosition| {
                let mut args = resulting_changeset_args.clone();
                let message = args.message + &format!(" #{}", num.0);
                args.message = message;
                args
            },
        )
        .await?;
        info!(
            ctx.logger(),
            "created {} commits, with the last commit {:?}",
            changesets.len(),
            changesets.last()
        );
    } else {
        perform_move(&ctx, &repo, parent_bcs_id, mover, resulting_changeset_args).await?;
    }
    Ok(())
}

async fn run_merge<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let first_parent = sub_m.value_of(FIRST_PARENT).unwrap().to_owned();
    let second_parent = sub_m.value_of(SECOND_PARENT).unwrap().to_owned();
    let resulting_changeset_args = cs_args_from_matches(sub_m);
    let (repo, resulting_changeset_args) = try_join(
        args::open_repo::<BlobRepo>(ctx.fb, &ctx.logger().clone(), matches),
        resulting_changeset_args.compat(),
    )
    .await?;

    let first_parent_fut = helpers::csid_resolve(&ctx, &repo, first_parent);
    let second_parent_fut = helpers::csid_resolve(&ctx, &repo, second_parent);
    let (first_parent, second_parent) = try_join(first_parent_fut, second_parent_fut).await?;

    info!(ctx.logger(), "Creating a merge commit");
    perform_merge(
        ctx.clone(),
        repo.clone(),
        first_parent,
        second_parent,
        resulting_changeset_args,
    )
    .await
    .map(|_| ())
}

async fn run_sync_diamond_merge<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let config_store = matches.config_store();
    let source_repo_id = args::get_source_repo_id(config_store, matches)?;
    let target_repo_id = args::get_target_repo_id(config_store, matches)?;
    let maybe_bookmark = sub_m
        .value_of(cli::COMMIT_BOOKMARK)
        .map(BookmarkName::new)
        .transpose()?;

    let bookmark = maybe_bookmark.ok_or_else(|| Error::msg("bookmark must be specified"))?;

    let source_repo = args::open_repo_with_repo_id(ctx.fb, ctx.logger(), source_repo_id, matches);
    let target_repo = args::open_repo_with_repo_id(ctx.fb, ctx.logger(), target_repo_id, matches);
    let mapping = args::open_source_sql::<SqlSyncedCommitMapping>(ctx.fb, config_store, matches)?;

    let merge_commit_hash = sub_m.value_of(COMMIT_HASH).unwrap().to_owned();
    let (source_repo, target_repo): (InnerRepo, BlobRepo) =
        try_join(source_repo, target_repo).await?;

    let source_merge_cs_id =
        helpers::csid_resolve(&ctx, &source_repo.blob_repo, merge_commit_hash).await?;

    let config_store = matches.config_store();
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(ctx.logger(), config_store)?;

    let caching = matches.caching();
    let x_repo_syncer_lease = create_commit_syncer_lease(ctx.fb, caching)?;

    sync_diamond_merge::do_sync_diamond_merge(
        ctx,
        source_repo,
        target_repo,
        source_merge_cs_id,
        mapping,
        bookmark,
        Arc::new(live_commit_sync_config),
        x_repo_syncer_lease,
    )
    .await
    .map(|_| ())
}

async fn run_pre_merge_delete<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo: BlobRepo = args::open_repo(ctx.fb, &ctx.logger().clone(), matches).await?;

    let delete_cs_args_factory = get_delete_commits_cs_args_factory(sub_m)?;

    let chunker = match sub_m.value_of(CHUNKING_HINT_FILE) {
        Some(hint_file) => {
            let hint_str = std::fs::read_to_string(hint_file)?;
            let hint = parse_chunking_hint(hint_str)?;
            path_chunker_from_hint(hint)?
        }
        None => {
            let even_chunk_size: usize = sub_m
                .value_of(EVEN_CHUNK_SIZE)
                .ok_or_else(|| {
                    format_err!(
                        "either {} or {} is required",
                        CHUNKING_HINT_FILE,
                        EVEN_CHUNK_SIZE
                    )
                })?
                .parse::<usize>()?;
            even_chunker_with_max_size(even_chunk_size)?
        }
    };

    let parent_bcs_id = {
        let hash = sub_m.value_of(COMMIT_HASH).unwrap().to_owned();
        helpers::csid_resolve(&ctx, &repo, hash).await?
    };

    let base_bcs_id = {
        match sub_m.value_of(BASE_COMMIT_HASH) {
            Some(hash) => {
                let bcs_id = helpers::csid_resolve(&ctx, &repo, hash).await?;
                Some(bcs_id)
            }
            None => None,
        }
    };
    let pmd = create_pre_merge_delete(
        &ctx,
        &repo,
        parent_bcs_id,
        chunker,
        delete_cs_args_factory,
        base_bcs_id,
    )
    .await?;

    let PreMergeDelete { mut delete_commits } = pmd;

    info!(
        ctx.logger(),
        "Listing deletion commits in top-to-bottom order (first commit is a descendant of the last)"
    );
    delete_commits.reverse();
    for delete_commit in delete_commits {
        println!("{}", delete_commit);
    }

    Ok(())
}

async fn run_history_fixup_delete<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo: BlobRepo = args::open_repo(ctx.fb, &ctx.logger().clone(), matches).await?;

    let delete_cs_args_factory = get_delete_commits_cs_args_factory(sub_m)?;

    let even_chunk_size: usize = sub_m
        .value_of(EVEN_CHUNK_SIZE)
        .ok_or_else(|| {
            format_err!(
                "either {} or {} is required",
                CHUNKING_HINT_FILE,
                EVEN_CHUNK_SIZE
            )
        })?
        .parse::<usize>()?;
    let chunker = even_chunker_with_max_size(even_chunk_size)?;

    let fixup_bcs_id = {
        let hash = sub_m.value_of(COMMIT_HASH).unwrap().to_owned();
        helpers::csid_resolve(&ctx, repo.clone(), hash).await?
    };

    let correct_bcs_id = {
        let hash = sub_m
            .value_of(COMMIT_HASH_CORRECT_HISTORY)
            .unwrap()
            .to_owned();
        helpers::csid_resolve(&ctx, repo.clone(), hash).await?
    };
    let paths_file = sub_m.value_of(PATHS_FILE).unwrap().to_owned();
    let s = read_to_string(&paths_file).await?;
    let paths: Vec<MPath> = s.lines().map(MPath::new).collect::<Result<Vec<MPath>>>()?;
    let hfd = create_history_fixup_deletes(
        &ctx,
        &repo,
        fixup_bcs_id,
        chunker,
        delete_cs_args_factory,
        correct_bcs_id,
        paths,
    )
    .await?;

    let HistoryFixupDeletes {
        mut delete_commits_fixup_branch,
        mut delete_commits_correct_branch,
    } = hfd;

    info!(
        ctx.logger(),
        "Listing deletion commits for fixup branch in top-to-bottom order (first commit is a descendant of the last)"
    );
    delete_commits_fixup_branch.reverse();
    for delete_commit in delete_commits_fixup_branch {
        println!("{}", delete_commit);
    }

    info!(
        ctx.logger(),
        "Listing deletion commits for branch with correct history in top-to-bottom order (first commit is a descendant of the last)"
    );
    delete_commits_correct_branch.reverse();
    for delete_commit in delete_commits_correct_branch {
        println!("{}", delete_commit);
    }

    Ok(())
}

async fn run_gradual_delete<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo: BlobRepo = args::open_repo(ctx.fb, &ctx.logger().clone(), matches).await?;

    let delete_cs_args_factory = get_delete_commits_cs_args_factory(sub_m)?;

    let chunker: Chunker<MPath> = {
        let even_chunk_size: usize = sub_m
            .value_of(EVEN_CHUNK_SIZE)
            .ok_or_else(|| format_err!("{} is required", EVEN_CHUNK_SIZE))?
            .parse::<usize>()?;
        even_chunker_with_max_size(even_chunk_size)?
    };

    let parent_bcs_id = {
        let hash = sub_m.value_of(COMMIT_HASH).unwrap().to_owned();
        helpers::csid_resolve(&ctx, &repo, hash).await?
    };

    let path_prefixes: Vec<_> = sub_m
        .values_of(PATH)
        .unwrap()
        .map(MPath::new)
        .collect::<Result<Vec<_>, Error>>()?;
    info!(
        ctx.logger(),
        "Gathering working copy files under {:?}", path_prefixes
    );
    let paths =
        get_working_copy_paths_by_prefixes(&ctx, &repo, parent_bcs_id, path_prefixes).await?;
    info!(ctx.logger(), "{} paths to be deleted", paths.len());

    info!(ctx.logger(), "Starting deletion");
    let delete_commits = delete_files_in_chunks(
        &ctx,
        &repo,
        parent_bcs_id,
        paths,
        &chunker,
        &delete_cs_args_factory,
        false, /* skip_last_chunk */
    )
    .await?;
    info!(ctx.logger(), "Deletion finished");
    info!(
        ctx.logger(),
        "Listing commits in an ancestor-descendant order"
    );
    for delete_commit in delete_commits {
        println!("{}", delete_commit);
    }

    Ok(())
}

async fn run_bonsai_merge<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo: BlobRepo = args::open_repo(ctx.fb, &ctx.logger().clone(), matches).await?;

    let (p1, p2) = try_join(
        async {
            let p1 = sub_m.value_of(BONSAI_MERGE_P1).unwrap().to_owned();
            helpers::csid_resolve(&ctx, &repo, p1).await
        },
        async {
            let p2 = sub_m.value_of(BONSAI_MERGE_P2).unwrap().to_owned();
            helpers::csid_resolve(&ctx, &repo, p2).await
        },
    )
    .await?;

    let cs_args = cs_args_from_matches(sub_m).compat().await?;

    let merge_cs_id =
        create_and_save_bonsai(&ctx, &repo, vec![p1, p2], Default::default(), cs_args).await?;

    println!("{}", merge_cs_id);

    Ok(())
}

async fn run_gradual_merge<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let config_store = matches.config_store();
    let repo: InnerRepo = args::open_repo(ctx.fb, ctx.logger(), matches).await?;

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
        args::get_config_by_repoid(config_store, matches, repo.blob_repo.get_repoid())?;
    let last_deletion_commit = helpers::csid_resolve(&ctx, &repo.blob_repo, last_deletion_commit);
    let pre_deletion_commit = helpers::csid_resolve(&ctx, &repo.blob_repo, pre_deletion_commit);

    let (last_deletion_commit, pre_deletion_commit) =
        try_join(last_deletion_commit, pre_deletion_commit).await?;

    let merge_changeset_args_factory = get_gradual_merge_commits_cs_args_factory(sub_m)?;
    let params = gradual_merge::GradualMergeParams {
        pre_deletion_commit,
        last_deletion_commit,
        bookmark_to_merge_into: BookmarkName::new(bookmark)?,
        merge_changeset_args_factory,
        limit,
        dry_run,
    };
    gradual_merge::gradual_merge(&ctx, &repo, &params, &repo_config.pushrebase.flags).await?;

    Ok(())
}

async fn run_gradual_merge_progress<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo: InnerRepo = args::open_repo(ctx.fb, ctx.logger(), matches).await?;

    let last_deletion_commit = sub_m
        .value_of(LAST_DELETION_COMMIT)
        .ok_or_else(|| format_err!("last deletion commit is not specified"))?;
    let pre_deletion_commit = sub_m
        .value_of(PRE_DELETION_COMMIT)
        .ok_or_else(|| format_err!("pre deletion commit is not specified"))?;
    let bookmark = sub_m
        .value_of(COMMIT_BOOKMARK)
        .ok_or_else(|| format_err!("bookmark where to merge is not specified"))?;

    let last_deletion_commit = helpers::csid_resolve(&ctx, &repo.blob_repo, last_deletion_commit);
    let pre_deletion_commit = helpers::csid_resolve(&ctx, &repo.blob_repo, pre_deletion_commit);

    let (last_deletion_commit, pre_deletion_commit) =
        try_join(last_deletion_commit, pre_deletion_commit).await?;

    let (done, total) = gradual_merge::gradual_merge_progress(
        &ctx,
        &repo,
        &pre_deletion_commit,
        &last_deletion_commit,
        &BookmarkName::new(bookmark)?,
    )
    .await?;

    println!("{}/{}", done, total);

    Ok(())
}

async fn run_manual_commit_sync<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer = create_commit_syncer_from_matches(&ctx, matches, None).await?;

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
                        .map(|p| helpers::csid_resolve(&ctx, target_repo, p)),
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
    let source_cs_id = helpers::csid_resolve(&ctx, source_repo, source_cs).await?;

    let mapping_version_name = sub_m
        .value_of(MAPPING_VERSION_NAME)
        .ok_or_else(|| format_err!("mapping-version-name is not specified"))?;

    let target_cs_id = manual_commit_sync::manual_commit_sync(
        &ctx,
        &commit_syncer,
        source_cs_id,
        target_repo_parents,
        CommitSyncConfigVersion(mapping_version_name.to_string()),
    )
    .await?;
    info!(ctx.logger(), "target cs id is {:?}", target_cs_id);
    Ok(())
}

async fn run_check_push_redirection_prereqs<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer = create_commit_syncer_from_matches(&ctx, matches, None).await?;

    let target_repo = commit_syncer.get_target_repo();
    let source_repo = commit_syncer.get_source_repo();

    info!(
        ctx.logger(),
        "Resolving source chageset in {}",
        source_repo.name()
    );
    let source_cs_id = helpers::csid_resolve(
        &ctx,
        source_repo,
        sub_m
            .value_of(SOURCE_CHANGESET)
            .ok_or_else(|| format_err!("{} not set", SOURCE_CHANGESET))?,
    )
    .await?;

    info!(
        ctx.logger(),
        "Resolving target changeset in {}",
        target_repo.name()
    );
    let target_cs_id = helpers::csid_resolve(
        &ctx,
        target_repo,
        sub_m
            .value_of(TARGET_CHANGESET)
            .ok_or_else(|| format_err!("{} not set", TARGET_CHANGESET))?,
    )
    .await?;

    let version = CommitSyncConfigVersion(
        sub_m
            .value_of(VERSION)
            .ok_or_else(|| format_err!("{} not set", VERSION))?
            .to_string(),
    );

    info!(
        ctx.logger(),
        "Checking push-redirection prerequisites for {}({})->{}({}), {:?}",
        source_cs_id,
        source_repo.name(),
        target_cs_id,
        target_repo.name(),
        version,
    );

    let config_store = matches.config_store();
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(ctx.logger(), config_store)?;
    verify_working_copy_with_version_fast_path(
        &ctx,
        &commit_syncer,
        Source(source_cs_id),
        Target(target_cs_id),
        &version,
        Arc::new(live_commit_sync_config),
    )
    .await
}

async fn run_catchup_delete_head<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo: BlobRepo = args::open_repo(ctx.fb, &ctx.logger().clone(), matches).await?;

    let head_bookmark = sub_m
        .value_of(HEAD_BOOKMARK)
        .ok_or_else(|| format_err!("{} not set", HEAD_BOOKMARK))?;

    let head_bookmark = BookmarkName::new(head_bookmark)?;

    let to_merge_cs_id = sub_m
        .value_of(TO_MERGE_CS_ID)
        .ok_or_else(|| format_err!("{} not set", TO_MERGE_CS_ID))?;
    let to_merge_cs_id = helpers::csid_resolve(&ctx, &repo, to_merge_cs_id).await?;

    let path_regex = sub_m
        .value_of(PATH_REGEX)
        .ok_or_else(|| format_err!("{} not set", PATH_REGEX))?;
    let path_regex = Regex::new(path_regex)?;

    let deletion_chunk_size = args::get_usize(&sub_m, DELETION_CHUNK_SIZE, 10000);

    let config_store = matches.config_store();
    let cs_args_factory = get_catchup_head_delete_commits_cs_args_factory(sub_m)?;
    let (_, repo_config) = args::get_config(config_store, matches)?;

    let wait_secs = args::get_u64(&sub_m, WAIT_SECS, 0);

    catchup::create_deletion_head_commits(
        &ctx,
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

async fn run_mover<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer = create_commit_syncer_from_matches(&ctx, matches, None).await?;
    let version = get_version(sub_m)?;
    let mover = commit_syncer.get_mover_by_version(&version).await?;
    let path = sub_m
        .value_of(PATH)
        .ok_or_else(|| format_err!("{} not set", PATH))?;
    let path = MPath::new(path)?;
    println!("{:?}", mover(&path));
    Ok(())
}

async fn run_catchup_validate<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo: BlobRepo = args::open_repo(ctx.fb, &ctx.logger().clone(), matches).await?;

    let result_commit = sub_m
        .value_of(COMMIT_HASH)
        .ok_or_else(|| format_err!("{} not set", COMMIT_HASH))?;
    let to_merge_cs_id = sub_m
        .value_of(TO_MERGE_CS_ID)
        .ok_or_else(|| format_err!("{} not set", TO_MERGE_CS_ID))?;
    let result_commit = helpers::csid_resolve(&ctx, &repo, result_commit);

    let to_merge_cs_id = helpers::csid_resolve(&ctx, &repo, to_merge_cs_id);

    let (result_commit, to_merge_cs_id) = try_join(result_commit, to_merge_cs_id).await?;

    let path_regex = sub_m
        .value_of(PATH_REGEX)
        .ok_or_else(|| format_err!("{} not set", PATH_REGEX))?;
    let path_regex = Regex::new(path_regex)?;

    catchup::validate(&ctx, &repo, result_commit, to_merge_cs_id, path_regex).await?;

    Ok(())
}

async fn run_mark_not_synced<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer = create_commit_syncer_from_matches(&ctx, matches, None).await?;

    let small_repo = commit_syncer.get_small_repo();
    let large_repo = commit_syncer.get_large_repo();
    let mapping = commit_syncer.get_mapping();

    let mapping_version_name = sub_m
        .value_of(MAPPING_VERSION_NAME)
        .ok_or_else(|| format_err!("{} is supposed to be set", MAPPING_VERSION_NAME))?;
    let mapping_version_name = CommitSyncConfigVersion(mapping_version_name.to_string());
    if !commit_syncer.version_exists(&mapping_version_name).await? {
        return Err(format_err!("{} version is not found", mapping_version_name));
    }

    let overwrite = sub_m.is_present(OVERWRITE);

    let input_file = sub_m
        .value_of(INPUT_FILE)
        .ok_or_else(|| format_err!("input-file is not specified"))?;
    let inputfile = File::open(&input_file)
        .await
        .with_context(|| format!("Failed to open {}", input_file))?;
    let reader = BufReader::new(inputfile);

    let ctx = &ctx;
    let mapping_version_name = &mapping_version_name;
    let s = tokio_stream::wrappers::LinesStream::new(reader.lines())
        .map_err(Error::from)
        .map_ok(move |line| async move {
            let cs_id = helpers::csid_resolve(ctx, large_repo, line).await?;

            let existing_value = mapping
                .get_equivalent_working_copy(
                    ctx,
                    large_repo.get_repoid(),
                    cs_id,
                    small_repo.get_repoid(),
                )
                .await?;

            if overwrite {
                if let Some(WorkingCopyEquivalence::WorkingCopy(_, _)) = existing_value {
                    return Err(format_err!("unexpected working copy found for {}", cs_id));
                }
            } else if existing_value.is_some() {
                info!(ctx.logger(), "{} already have mapping", cs_id);
                return Ok(1);
            }

            let wc_entry = EquivalentWorkingCopyEntry {
                large_repo_id: large_repo.get_repoid(),
                large_bcs_id: cs_id,
                small_repo_id: small_repo.get_repoid(),
                small_bcs_id: None,
                version_name: Some(mapping_version_name.clone()),
            };
            let res = if overwrite {
                mapping
                    .overwrite_equivalent_working_copy(ctx, wc_entry)
                    .await?
            } else {
                mapping
                    .insert_equivalent_working_copy(ctx, wc_entry)
                    .await?
            };
            if !res {
                warn!(
                    ctx.logger(),
                    "failed to insert NotSyncedMapping entry for {}", cs_id
                );
            }

            // Processed a single entry
            Ok(1)
        })
        .try_buffer_unordered(100);

    process_stream_and_wait_for_replication(ctx, matches, &commit_syncer, s).await?;

    Ok(())
}

async fn run_backfill_noop_mapping<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer = create_commit_syncer_from_matches(&ctx, matches, None).await?;

    let small_repo = commit_syncer.get_small_repo();
    let large_repo = commit_syncer.get_large_repo();

    info!(
        ctx.logger(),
        "small repo: {}, large repo: {}",
        small_repo.name(),
        large_repo.name(),
    );
    let mapping_version_name = sub_m
        .value_of(MAPPING_VERSION_NAME)
        .ok_or_else(|| format_err!("mapping-version-name is not specified"))?;
    let mapping_version_name = CommitSyncConfigVersion(mapping_version_name.to_string());
    if !commit_syncer.version_exists(&mapping_version_name).await? {
        return Err(format_err!("{} version is not found", mapping_version_name));
    }

    let input_file = sub_m
        .value_of(INPUT_FILE)
        .ok_or_else(|| format_err!("input-file is not specified"))?;

    let inputfile = File::open(&input_file)
        .await
        .with_context(|| format!("Failed to open {}", input_file))?;
    let reader = BufReader::new(inputfile);

    let s = tokio_stream::wrappers::LinesStream::new(reader.lines())
        .map_err(Error::from)
        .map_ok({
            borrowed!(ctx, commit_syncer, mapping_version_name);
            move |cs_id| async move {
                let small_cs_id = helpers::csid_resolve(ctx, small_repo, cs_id.clone());

                let large_cs_id = helpers::csid_resolve(ctx, large_repo, cs_id);

                let (small_cs_id, large_cs_id) = try_join(small_cs_id, large_cs_id).await?;

                let entry = SyncedCommitMappingEntry {
                    large_repo_id: large_repo.get_repoid(),
                    large_bcs_id: large_cs_id,
                    small_repo_id: small_repo.get_repoid(),
                    small_bcs_id: small_cs_id,
                    version_name: Some(mapping_version_name.clone()),
                    source_repo: Some(commit_syncer.get_source_repo_type()),
                };
                Ok(entry)
            }
        })
        .try_buffer_unordered(100)
        .chunks(100)
        .then({
            borrowed!(commit_syncer, ctx);
            move |chunk| async move {
                let mapping = &commit_syncer.mapping;
                let chunk: Result<Vec<_>, Error> = chunk.into_iter().collect();
                let chunk = chunk?;
                let len = chunk.len();
                mapping.add_bulk(ctx, chunk).await?;
                Result::<_, Error>::Ok(len as u64)
            }
        })
        .boxed();

    process_stream_and_wait_for_replication(&ctx, matches, &commit_syncer, s).await?;

    Ok(())
}

async fn run_diff_mapping_versions<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let config_store = matches.config_store();
    let source_repo_id = args::get_source_repo_id(config_store, matches)?;
    let target_repo_id = args::get_target_repo_id(config_store, matches)?;

    let mapping_version_names = sub_m
        .values_of(MAPPING_VERSION_NAME)
        .ok_or_else(|| format_err!("{} is supposed to be set", MAPPING_VERSION_NAME))?;

    let config_store = matches.config_store();
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(ctx.logger(), config_store)?;

    let mut commit_sync_configs = vec![];
    for version in mapping_version_names {
        let version = CommitSyncConfigVersion(version.to_string());
        let config = live_commit_sync_config
            .get_commit_sync_config_by_version(target_repo_id, &version)
            .await?;
        commit_sync_configs.push(config);
    }

    if commit_sync_configs.len() != 2 {
        return Err(format_err!(
            "{} should have exactly 2 values",
            MAPPING_VERSION_NAME
        ));
    }

    // Validate that both versions related to the same config.
    let from = commit_sync_configs.remove(0);
    let to = commit_sync_configs.remove(0);
    if from.large_repo_id != to.large_repo_id {
        return Err(format_err!(
            "different large repo ids: {} vs {}",
            from.large_repo_id,
            to.large_repo_id
        ));
    }

    let small_repo_id = if from.large_repo_id == target_repo_id {
        source_repo_id
    } else {
        target_repo_id
    };

    if !from.small_repos.contains_key(&small_repo_id) {
        return Err(format_err!(
            "{} doesn't have small repo id {}",
            from.version_name,
            small_repo_id,
        ));
    }

    if !to.small_repos.contains_key(&small_repo_id) {
        return Err(format_err!(
            "{} doesn't have small repo id {}",
            to.version_name,
            small_repo_id,
        ));
    }

    let from_small_commit_sync_config = from
        .small_repos
        .get(&small_repo_id)
        .cloned()
        .ok_or_else(|| format_err!("{} not found in {}", small_repo_id, from.version_name))?;
    let to_small_commit_sync_config = to
        .small_repos
        .get(&small_repo_id)
        .cloned()
        .ok_or_else(|| format_err!("{} not found in {}", small_repo_id, to.version_name))?;

    let diff = diff_small_repo_commit_sync_configs(
        from_small_commit_sync_config,
        to_small_commit_sync_config,
    );

    if let Some((from, to)) = diff.default_action_change {
        println!("default action change: {:?} to {:?}", from, to);
    }

    let mut mapping_added = diff.mapping_added.into_iter().collect::<Vec<_>>();
    mapping_added.sort();
    for (path_from, path_to) in mapping_added {
        println!("mapping added: {} => {}", path_from, path_to);
    }

    let mut mapping_changed = diff.mapping_changed.into_iter().collect::<Vec<_>>();
    mapping_changed.sort();
    for (path_from, (before, after)) in mapping_changed {
        println!("mapping changed: {} => {} vs {}", path_from, before, after);
    }

    let mut mapping_removed = diff.mapping_removed.into_iter().collect::<Vec<_>>();
    mapping_removed.sort();
    for (path_from, path_to) in mapping_removed {
        println!("mapping removed: {} => {}", path_from, path_to);
    }

    Ok(())
}

async fn process_stream_and_wait_for_replication<'a>(
    ctx: &CoreContext,
    matches: &MononokeMatches<'a>,
    commit_syncer: &CommitSyncer<SqlSyncedCommitMapping>,
    mut s: impl Stream<Item = Result<u64>> + std::marker::Unpin,
) -> Result<(), Error> {
    let config_store = matches.config_store();
    let small_repo = commit_syncer.get_small_repo();
    let large_repo = commit_syncer.get_large_repo();

    let (_, small_repo_config) =
        args::get_config_by_repoid(config_store, matches, small_repo.get_repoid())?;
    let (_, large_repo_config) =
        args::get_config_by_repoid(config_store, matches, large_repo.get_repoid())?;
    if small_repo_config.storage_config.metadata != large_repo_config.storage_config.metadata {
        return Err(format_err!(
            "{} and {} have different db metadata configs: {:?} vs {:?}",
            small_repo.name(),
            large_repo.name(),
            small_repo_config.storage_config.metadata,
            large_repo_config.storage_config.metadata,
        ));
    }
    let storage_config = small_repo_config.storage_config;

    let db_address = match &storage_config.metadata {
        MetadataDatabaseConfig::Local(_) => None,
        MetadataDatabaseConfig::Remote(remote_config) => {
            Some(remote_config.primary.db_address.clone())
        }
    };

    let wait_config = WaitForReplicationConfig::default().with_logger(ctx.logger());
    let replica_lag_monitor: Arc<dyn ReplicaLagMonitor> = match db_address {
        None => Arc::new(NoReplicaLagMonitor()),
        Some(address) => {
            #[cfg(fbcode_build)]
            {
                let my_admin = MyAdmin::new(ctx.fb).context("building myadmin client")?;
                Arc::new(my_admin.single_shard_lag_monitor(address))
            }
            #[cfg(not(fbcode_build))]
            {
                let _address = address;
                Arc::new(NoReplicaLagMonitor())
            }
        }
    };

    let mut total = 0;
    let mut batch = 0;
    while let Some(chunk_size) = s.try_next().await? {
        total += chunk_size;

        batch += chunk_size;
        if batch < 100 {
            continue;
        }
        info!(ctx.logger(), "processed {} changesets", total);
        batch %= 100;
        replica_lag_monitor
            .wait_for_replication(&wait_config)
            .await?;
    }

    Ok(())
}

async fn run_sync_commit_and_ancestors<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer = create_commit_syncer_from_matches(&ctx, matches, None).await?;

    let source_commit_hash = sub_m
        .value_of(COMMIT_HASH)
        .ok_or_else(|| format_err!("{} not specified", COMMIT_HASH))?;

    let source_cs_id =
        helpers::csid_resolve(&ctx, commit_syncer.get_source_repo(), source_commit_hash).await?;

    let (unsynced_ancestors, _) =
        find_toposorted_unsynced_ancestors(&ctx, &commit_syncer, source_cs_id).await?;

    for ancestor in unsynced_ancestors {
        commit_syncer
            .unsafe_sync_commit(
                &ctx,
                ancestor,
                CandidateSelectionHint::Only,
                CommitSyncContext::AdminChangeMapping,
            )
            .await?;
    }

    let commit_sync_outcome = commit_syncer
        .get_commit_sync_outcome(&ctx, source_cs_id)
        .await?
        .ok_or_else(|| format_err!("was not able to remap a commit {}", source_cs_id))?;
    info!(ctx.logger(), "remapped to {:?}", commit_sync_outcome);

    Ok(())
}

fn get_version(matches: &ArgMatches<'_>) -> Result<CommitSyncConfigVersion> {
    Ok(CommitSyncConfigVersion(
        matches
            .value_of(VERSION)
            .ok_or_else(|| format_err!("{} not set", VERSION))?
            .to_string(),
    ))
}

async fn run_delete_no_longer_bound_files_from_large_repo<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer = create_commit_syncer_from_matches(&ctx, matches, None).await?;
    let large_repo = commit_syncer.get_large_repo();
    if commit_syncer.get_source_repo().get_repoid() != large_repo.get_repoid() {
        return Err(format_err!("source repo must be large!"));
    }

    let cs_id = sub_m
        .value_of(COMMIT_HASH)
        .ok_or_else(|| format_err!("{} not specified", COMMIT_HASH))?;
    let cs_id = helpers::csid_resolve(&ctx, commit_syncer.get_source_repo(), cs_id).await?;

    // Find all files under a given path
    let prefix = sub_m.value_of(PATH_PREFIX).context("prefix is not set")?;
    let root_fsnode_id = RootFsnodeId::derive(&ctx, large_repo, cs_id).await?;
    let entries = root_fsnode_id
        .fsnode_id()
        .find_entries(
            ctx.clone(),
            large_repo.get_blobstore(),
            vec![PathOrPrefix::Prefix(Some(MPath::new(prefix)?))],
        )
        .try_collect::<Vec<_>>()
        .await?;

    // Now find which files does not remap to a small repo - these files we want to delete
    let mover = find_mover_for_commit(&ctx, &commit_syncer, cs_id).await?;

    let mut to_delete = vec![];
    for (path, entry) in entries {
        if let Entry::Leaf(_) = entry {
            let path = path.unwrap();
            if mover(&path)?.is_none() {
                to_delete.push(path);
            }
        }
    }

    if to_delete.is_empty() {
        info!(ctx.logger(), "nothing to delete, exiting");
        return Ok(());
    }
    info!(ctx.logger(), "need to delete {} paths", to_delete.len());

    let resulting_changeset_args = cs_args_from_matches(sub_m).compat().await?;
    let deletion_cs_id = create_and_save_bonsai(
        &ctx,
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

async fn find_mover_for_commit(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncer<SqlSyncedCommitMapping>,
    cs_id: ChangesetId,
) -> Result<Mover, Error> {
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
            commit_syncer.get_mover_by_version(&version).await?
        }
    };

    Ok(mover)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = setup_app();
    let matches = app.get_matches(fb)?;
    let logger = matches.logger();
    let config_store = matches.config_store();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let subcommand_future = async {
        match matches.subcommand() {
            (BACKFILL_NOOP_MAPPING, Some(sub_m)) => {
                run_backfill_noop_mapping(ctx, &matches, sub_m).await
            }
            (BONSAI_MERGE, Some(sub_m)) => run_bonsai_merge(ctx, &matches, sub_m).await,
            (CHECK_PUSH_REDIRECTION_PREREQS, Some(sub_m)) => {
                run_check_push_redirection_prereqs(ctx, &matches, sub_m).await
            }
            (DIFF_MAPPING_VERSIONS, Some(sub_m)) => {
                run_diff_mapping_versions(ctx, &matches, sub_m).await
            }
            (MANUAL_COMMIT_SYNC, Some(sub_m)) => run_manual_commit_sync(ctx, &matches, sub_m).await,
            (MARK_NOT_SYNCED_COMMAND, Some(sub_m)) => {
                run_mark_not_synced(ctx, &matches, sub_m).await
            }
            (MERGE, Some(sub_m)) => run_merge(ctx, &matches, sub_m).await,
            (MOVE, Some(sub_m)) => {
                let live_commit_sync_config =
                    CfgrLiveCommitSyncConfig::new(ctx.logger(), config_store)?;
                run_move(ctx, &matches, sub_m, live_commit_sync_config).await
            }
            (RUN_MOVER, Some(sub_m)) => run_mover(ctx, &matches, sub_m).await,
            (SYNC_COMMIT_AND_ANCESTORS, Some(sub_m)) => {
                run_sync_commit_and_ancestors(ctx, &matches, sub_m).await
            }
            (SYNC_DIAMOND_MERGE, Some(sub_m)) => run_sync_diamond_merge(ctx, &matches, sub_m).await,

            // All commands relevant to gradual merge
            (CATCHUP_DELETE_HEAD, Some(sub_m)) => {
                run_catchup_delete_head(ctx, &matches, sub_m).await
            }
            (CATCHUP_VALIDATE_COMMAND, Some(sub_m)) => {
                run_catchup_validate(ctx, &matches, sub_m).await
            }
            (GRADUAL_DELETE, Some(sub_m)) => run_gradual_delete(ctx, &matches, sub_m).await,
            (GRADUAL_MERGE, Some(sub_m)) => run_gradual_merge(ctx, &matches, sub_m).await,
            (GRADUAL_MERGE_PROGRESS, Some(sub_m)) => {
                run_gradual_merge_progress(ctx, &matches, sub_m).await
            }
            (PRE_MERGE_DELETE, Some(sub_m)) => run_pre_merge_delete(ctx, &matches, sub_m).await,
            (HISTORY_FIXUP_DELETE, Some(sub_m)) => {
                run_history_fixup_delete(ctx, &matches, sub_m).await
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
