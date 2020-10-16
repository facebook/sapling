/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![feature(process_exitcode_placeholder)]

use anyhow::{bail, format_err, Context, Error, Result};
use bookmarks::BookmarkName;
use borrowed::borrowed;
use clap::ArgMatches;
use cmdlib::{args, helpers};
use cmdlib_x_repo::create_commit_syncer_args_from_matches;
use context::CoreContext;
use cross_repo_sync::CommitSyncer;
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    future::{try_join, try_join3, try_join_all},
    Stream, StreamExt, TryStreamExt,
};
use live_commit_sync_config::{CfgrLiveCommitSyncConfig, LiveCommitSyncConfig};
use metaconfig_types::RepoConfig;
use metaconfig_types::{CommitSyncConfigVersion, MetadataDatabaseConfig};
use mononoke_types::{MPath, RepositoryId};
use movers::get_small_to_large_mover;
use regex::Regex;
use skiplist::fetch_skiplist_index;
use slog::{info, warn};
#[cfg(fbcode_build)]
use sql_ext::facebook::MyAdmin;
use sql_ext::replication::{NoReplicaLagMonitor, ReplicaLagMonitor, WaitForReplicationConfig};
use std::collections::BTreeMap;
use std::num::NonZeroU64;
use std::sync::Arc;
use synced_commit_mapping::{
    EquivalentWorkingCopyEntry, SqlSyncedCommitMapping, SyncedCommitMapping,
    SyncedCommitMappingEntry,
};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, BufReader},
};

mod catchup;
mod cli;
mod gradual_merge;
mod manual_commit_sync;
mod merging;
mod sync_diamond_merge;

use crate::cli::{
    cs_args_from_matches, get_catchup_head_delete_commits_cs_args_factory,
    get_delete_commits_cs_args_factory, get_gradual_merge_commits_cs_args_factory, setup_app,
    BACKFILL_NOOP_MAPPING, BASE_COMMIT_HASH, BONSAI_MERGE, BONSAI_MERGE_P1, BONSAI_MERGE_P2,
    CATCHUP_DELETE_HEAD, CATCHUP_VALIDATE_COMMAND, CHANGESET, CHUNKING_HINT_FILE, COMMIT_BOOKMARK,
    COMMIT_HASH, DELETION_CHUNK_SIZE, DRY_RUN, EVEN_CHUNK_SIZE, FIRST_PARENT, GRADUAL_MERGE,
    GRADUAL_MERGE_PROGRESS, HEAD_BOOKMARK, INPUT_FILE, LAST_DELETION_COMMIT, LIMIT,
    MANUAL_COMMIT_SYNC, MAPPING_VERSION_NAME, MARK_NOT_SYNCED_COMMAND, MAX_NUM_OF_MOVES_IN_COMMIT,
    MERGE, MOVE, ORIGIN_REPO, PARENTS, PATH, PATH_REGEX, PRE_DELETION_COMMIT, PRE_MERGE_DELETE,
    RUN_MOVER, SECOND_PARENT, SYNC_DIAMOND_MERGE, TO_MERGE_CS_ID, VERSION, WAIT_SECS,
};
use crate::merging::perform_merge;
use megarepolib::chunking::{
    even_chunker_with_max_size, parse_chunking_hint, path_chunker_from_hint,
};
use megarepolib::common::create_and_save_bonsai;
use megarepolib::pre_merge_delete::{create_pre_merge_delete, PreMergeDelete};
use megarepolib::{common::StackPosition, perform_move, perform_stack_move};

async fn run_move<'a>(
    ctx: CoreContext,
    matches: &ArgMatches<'a>,
    sub_m: &ArgMatches<'a>,
    repo_config: RepoConfig,
) -> Result<(), Error> {
    let origin_repo =
        RepositoryId::new(args::get_i32_opt(&sub_m, ORIGIN_REPO).expect("Origin repo is missing"));
    let resulting_changeset_args = cs_args_from_matches(&sub_m);
    let commit_sync_config = repo_config.commit_sync_config.as_ref().unwrap();
    let mover = get_small_to_large_mover(commit_sync_config, origin_repo).unwrap();
    let move_parent = sub_m.value_of(CHANGESET).unwrap().to_owned();

    let max_num_of_moves_in_commit =
        args::get_and_parse_opt::<NonZeroU64>(sub_m, MAX_NUM_OF_MOVES_IN_COMMIT);

    let (repo, resulting_changeset_args) = try_join(
        args::open_repo(ctx.fb, &ctx.logger().clone(), &matches).compat(),
        resulting_changeset_args.compat(),
    )
    .await?;

    let parent_bcs_id = helpers::csid_resolve(ctx.clone(), repo.clone(), move_parent)
        .compat()
        .await?;

    if let Some(max_num_of_moves_in_commit) = max_num_of_moves_in_commit {
        perform_stack_move(
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
        .await
        .map(|changesets| {
            info!(
                ctx.logger(),
                "created {} commits, with the last commit {:?}",
                changesets.len(),
                changesets.last()
            );
            ()
        })
    } else {
        perform_move(&ctx, &repo, parent_bcs_id, mover, resulting_changeset_args)
            .await
            .map(|_| ())
    }
}

async fn run_merge<'a>(
    ctx: CoreContext,
    matches: &ArgMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let first_parent = sub_m.value_of(FIRST_PARENT).unwrap().to_owned();
    let second_parent = sub_m.value_of(SECOND_PARENT).unwrap().to_owned();
    let resulting_changeset_args = cs_args_from_matches(&sub_m);
    let (repo, resulting_changeset_args) = try_join(
        args::open_repo(ctx.fb, &ctx.logger().clone(), &matches).compat(),
        resulting_changeset_args.compat(),
    )
    .await?;

    let first_parent_fut = helpers::csid_resolve(ctx.clone(), repo.clone(), first_parent);
    let second_parent_fut = helpers::csid_resolve(ctx.clone(), repo.clone(), second_parent);
    let (first_parent, second_parent) =
        try_join(first_parent_fut.compat(), second_parent_fut.compat()).await?;

    info!(ctx.logger(), "Creating a merge commit");
    perform_merge(
        ctx.clone(),
        repo.clone(),
        first_parent,
        second_parent,
        resulting_changeset_args,
    )
    .compat()
    .await
    .map(|_| ())
}

async fn run_sync_diamond_merge<'a>(
    ctx: CoreContext,
    matches: &ArgMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let source_repo_id = args::get_source_repo_id(ctx.fb, matches)?;
    let target_repo_id = args::get_target_repo_id(ctx.fb, matches)?;
    let maybe_bookmark = sub_m
        .value_of(cli::COMMIT_BOOKMARK)
        .map(|bookmark_str| BookmarkName::new(bookmark_str))
        .transpose()?;

    let bookmark = maybe_bookmark.ok_or(Error::msg("bookmark must be specified"))?;

    let source_repo = args::open_repo_with_repo_id(ctx.fb, ctx.logger(), source_repo_id, matches);
    let target_repo = args::open_repo_with_repo_id(ctx.fb, ctx.logger(), target_repo_id, matches);
    let mapping = args::open_source_sql::<SqlSyncedCommitMapping>(ctx.fb, &matches);

    let (_, source_repo_config) = args::get_config_by_repoid(ctx.fb, matches, source_repo_id)?;

    let merge_commit_hash = sub_m.value_of(COMMIT_HASH).unwrap().to_owned();
    let (source_repo, target_repo, mapping) =
        try_join3(source_repo.compat(), target_repo.compat(), mapping.compat()).await?;

    let source_merge_cs_id =
        helpers::csid_resolve(ctx.clone(), source_repo.clone(), merge_commit_hash)
            .compat()
            .await?;

    let config_store = args::maybe_init_config_store(ctx.fb, ctx.logger(), &matches)
        .ok_or_else(|| format_err!("Failed initializing ConfigStore"))?;
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(ctx.logger(), &config_store)?;
    sync_diamond_merge::do_sync_diamond_merge(
        ctx,
        source_repo,
        target_repo,
        source_merge_cs_id,
        mapping,
        source_repo_config,
        bookmark,
        Arc::new(live_commit_sync_config),
    )
    .await
    .map(|_| ())
}

async fn run_pre_merge_delete<'a>(
    ctx: CoreContext,
    matches: &ArgMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo = args::open_repo(ctx.fb, &ctx.logger().clone(), &matches)
        .compat()
        .await?;

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
        helpers::csid_resolve(ctx.clone(), repo.clone(), hash)
            .compat()
            .await?
    };

    let base_bcs_id = {
        match sub_m.value_of(BASE_COMMIT_HASH) {
            Some(hash) => {
                let bcs_id = helpers::csid_resolve(ctx.clone(), repo.clone(), hash)
                    .compat()
                    .await?;
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

async fn run_bonsai_merge<'a>(
    ctx: CoreContext,
    matches: &ArgMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo = args::open_repo(ctx.fb, &ctx.logger().clone(), &matches)
        .compat()
        .await?;

    let (p1, p2) = try_join(
        async {
            let p1 = sub_m.value_of(BONSAI_MERGE_P1).unwrap().to_owned();
            helpers::csid_resolve(ctx.clone(), repo.clone(), p1)
                .compat()
                .await
        },
        async {
            let p2 = sub_m.value_of(BONSAI_MERGE_P2).unwrap().to_owned();
            helpers::csid_resolve(ctx.clone(), repo.clone(), p2)
                .compat()
                .await
        },
    )
    .await?;

    let cs_args = cs_args_from_matches(sub_m).compat().await?;

    let merge_cs_id =
        create_and_save_bonsai(&ctx, &repo, vec![p1, p2], BTreeMap::new(), cs_args).await?;

    println!("{}", merge_cs_id);

    Ok(())
}

async fn run_gradual_merge<'a>(
    ctx: CoreContext,
    matches: &ArgMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo = args::open_repo(ctx.fb, &ctx.logger(), &matches)
        .compat()
        .await?;

    let last_deletion_commit = sub_m
        .value_of(LAST_DELETION_COMMIT)
        .ok_or(format_err!("last deletion commit is not specified"))?;
    let pre_deletion_commit = sub_m
        .value_of(PRE_DELETION_COMMIT)
        .ok_or(format_err!("pre deletion commit is not specified"))?;
    let bookmark = sub_m
        .value_of(COMMIT_BOOKMARK)
        .ok_or(format_err!("bookmark where to merge is not specified"))?;
    let dry_run = sub_m.is_present(DRY_RUN);

    let limit = args::get_usize_opt(sub_m, LIMIT);
    let (_, repo_config) = args::get_config_by_repoid(ctx.fb, &matches, repo.get_repoid())?;
    let last_deletion_commit =
        helpers::csid_resolve(ctx.clone(), repo.clone(), last_deletion_commit).compat();
    let pre_deletion_commit =
        helpers::csid_resolve(ctx.clone(), repo.clone(), pre_deletion_commit).compat();

    let blobstore = repo.get_blobstore().boxed();
    let skiplist =
        fetch_skiplist_index(&ctx, &repo_config.skiplist_index_blobstore_key, &blobstore);

    let (last_deletion_commit, pre_deletion_commit, skiplist) =
        try_join3(last_deletion_commit, pre_deletion_commit, skiplist).await?;

    let merge_changeset_args_factory = get_gradual_merge_commits_cs_args_factory(&sub_m)?;
    let params = gradual_merge::GradualMergeParams {
        pre_deletion_commit,
        last_deletion_commit,
        bookmark_to_merge_into: BookmarkName::new(bookmark)?,
        merge_changeset_args_factory,
        limit,
        dry_run,
    };
    gradual_merge::gradual_merge(
        &ctx,
        &repo,
        &skiplist,
        &params,
        &repo_config.pushrebase.flags,
    )
    .await?;

    Ok(())
}

async fn run_gradual_merge_progress<'a>(
    ctx: CoreContext,
    matches: &ArgMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo = args::open_repo(ctx.fb, &ctx.logger(), &matches)
        .compat()
        .await?;

    let last_deletion_commit = sub_m
        .value_of(LAST_DELETION_COMMIT)
        .ok_or(format_err!("last deletion commit is not specified"))?;
    let pre_deletion_commit = sub_m
        .value_of(PRE_DELETION_COMMIT)
        .ok_or(format_err!("pre deletion commit is not specified"))?;
    let bookmark = sub_m
        .value_of(COMMIT_BOOKMARK)
        .ok_or(format_err!("bookmark where to merge is not specified"))?;

    let (_, repo_config) = args::get_config_by_repoid(ctx.fb, &matches, repo.get_repoid())?;
    let last_deletion_commit =
        helpers::csid_resolve(ctx.clone(), repo.clone(), last_deletion_commit).compat();
    let pre_deletion_commit =
        helpers::csid_resolve(ctx.clone(), repo.clone(), pre_deletion_commit).compat();

    let blobstore = repo.get_blobstore().boxed();
    let skiplist =
        fetch_skiplist_index(&ctx, &repo_config.skiplist_index_blobstore_key, &blobstore);

    let (last_deletion_commit, pre_deletion_commit, skiplist) =
        try_join3(last_deletion_commit, pre_deletion_commit, skiplist).await?;

    let (done, total) = gradual_merge::gradual_merge_progress(
        &ctx,
        &repo,
        &skiplist,
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
    matches: &ArgMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer = get_commit_syncer(&ctx, matches).await?;

    let target_repo = commit_syncer.get_target_repo();
    let target_repo_parents = sub_m.values_of(PARENTS);
    let target_repo_parents = match target_repo_parents {
        Some(target_repo_parents) => {
            try_join_all(
                target_repo_parents
                    .into_iter()
                    .map(|p| helpers::csid_resolve(ctx.clone(), target_repo.clone(), p).compat()),
            )
            .await?
        }
        None => vec![],
    };

    let source_cs = sub_m
        .value_of(CHANGESET)
        .ok_or_else(|| format_err!("{} not set", CHANGESET))?;
    let source_repo = commit_syncer.get_source_repo();
    let source_cs_id = helpers::csid_resolve(ctx.clone(), source_repo.clone(), source_cs)
        .compat()
        .await?;

    let target_cs_id = manual_commit_sync::manual_commit_sync(
        &ctx,
        &commit_syncer,
        source_cs_id,
        target_repo_parents,
    )
    .await?;
    info!(ctx.logger(), "target cs id is {:?}", target_cs_id);
    Ok(())
}

async fn run_catchup_delete_head<'a>(
    ctx: CoreContext,
    matches: &ArgMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo = args::open_repo(ctx.fb, &ctx.logger().clone(), &matches)
        .compat()
        .await?;

    let head_bookmark = sub_m
        .value_of(HEAD_BOOKMARK)
        .ok_or_else(|| format_err!("{} not set", HEAD_BOOKMARK))?;

    let head_bookmark = BookmarkName::new(head_bookmark)?;

    let to_merge_cs_id = sub_m
        .value_of(TO_MERGE_CS_ID)
        .ok_or_else(|| format_err!("{} not set", TO_MERGE_CS_ID))?;
    let to_merge_cs_id = helpers::csid_resolve(ctx.clone(), repo.clone(), to_merge_cs_id)
        .compat()
        .await?;

    let path_regex = sub_m
        .value_of(PATH_REGEX)
        .ok_or_else(|| format_err!("{} not set", PATH_REGEX))?;
    let path_regex = Regex::new(path_regex)?;

    let deletion_chunk_size = args::get_usize(&sub_m, DELETION_CHUNK_SIZE, 10000);

    let cs_args_factory = get_catchup_head_delete_commits_cs_args_factory(&sub_m)?;
    let (_, repo_config) = args::get_config(ctx.fb, &matches)?;

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
    matches: &ArgMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer = get_commit_syncer(&ctx, matches).await?;
    let version = get_version(sub_m)?;
    let mover = commit_syncer.get_mover_by_version(&version)?;
    let path = sub_m
        .value_of(PATH)
        .ok_or_else(|| format_err!("{} not set", PATH))?;
    let path = MPath::new(path)?;
    println!("{:?}", mover(&path));
    Ok(())
}

async fn run_catchup_validate<'a>(
    ctx: CoreContext,
    matches: &ArgMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let repo = args::open_repo(ctx.fb, &ctx.logger().clone(), &matches)
        .compat()
        .await?;

    let result_commit = sub_m
        .value_of(COMMIT_HASH)
        .ok_or_else(|| format_err!("{} not set", COMMIT_HASH))?;
    let to_merge_cs_id = sub_m
        .value_of(TO_MERGE_CS_ID)
        .ok_or_else(|| format_err!("{} not set", TO_MERGE_CS_ID))?;
    let result_commit = helpers::csid_resolve(ctx.clone(), repo.clone(), result_commit).compat();

    let to_merge_cs_id = helpers::csid_resolve(ctx.clone(), repo.clone(), to_merge_cs_id).compat();

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
    matches: &ArgMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer = get_commit_syncer(&ctx, matches).await?;

    let small_repo = commit_syncer.get_small_repo();
    let large_repo = commit_syncer.get_large_repo();
    let mapping = commit_syncer.get_mapping();

    let input_file = sub_m
        .value_of(INPUT_FILE)
        .ok_or_else(|| format_err!("input-file is not specified"))?;
    let inputfile = File::open(&input_file)
        .await
        .with_context(|| format!("Failed to open {}", input_file))?;
    let reader = BufReader::new(inputfile);

    let ctx = &ctx;
    let s = reader
        .lines()
        .map_err(Error::from)
        .map_ok(move |line| async move {
            let cs_id = helpers::csid_resolve(ctx.clone(), large_repo.clone(), line)
                .compat()
                .await?;
            let mappings = mapping
                .get(
                    ctx.clone(),
                    large_repo.get_repoid(),
                    cs_id,
                    small_repo.get_repoid(),
                )
                .compat()
                .await?;
            if mappings.is_empty() {
                let wc_entry = EquivalentWorkingCopyEntry {
                    large_repo_id: large_repo.get_repoid(),
                    large_bcs_id: cs_id,
                    small_repo_id: small_repo.get_repoid(),
                    small_bcs_id: None,
                    version_name: None,
                };
                let res = mapping
                    .insert_equivalent_working_copy(ctx.clone(), wc_entry)
                    .compat()
                    .await?;
                if !res {
                    warn!(
                        ctx.logger(),
                        "failed to insert NotSyncedMapping entry for {}", cs_id
                    );
                }
            } else {
                info!(ctx.logger(), "{} already have mapping", cs_id);
            }

            // Processed a single entry
            Ok(1)
        })
        .try_buffer_unordered(100);

    process_stream_and_wait_for_replication(&ctx, matches, &commit_syncer, s).await?;

    Ok(())
}

async fn run_backfill_noop_mapping<'a>(
    ctx: CoreContext,
    matches: &ArgMatches<'a>,
    sub_m: &ArgMatches<'a>,
) -> Result<(), Error> {
    let commit_syncer = get_commit_syncer(&ctx, matches).await?;

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
    if !commit_syncer.version_exists(&mapping_version_name)? {
        return Err(format_err!("{} version is not found", mapping_version_name));
    }

    let input_file = sub_m
        .value_of(INPUT_FILE)
        .ok_or_else(|| format_err!("input-file is not specified"))?;

    let inputfile = File::open(&input_file)
        .await
        .with_context(|| format!("Failed to open {}", input_file))?;
    let reader = BufReader::new(inputfile);

    let s = reader
        .lines()
        .map_err(Error::from)
        .map_ok({
            borrowed!(ctx, mapping_version_name);
            move |cs_id| async move {
                let small_cs_id =
                    helpers::csid_resolve(ctx.clone(), small_repo.clone(), cs_id.clone()).compat();

                let large_cs_id =
                    helpers::csid_resolve(ctx.clone(), large_repo.clone(), cs_id).compat();

                let (small_cs_id, large_cs_id) = try_join(small_cs_id, large_cs_id).await?;

                let entry = SyncedCommitMappingEntry {
                    large_repo_id: large_repo.get_repoid(),
                    large_bcs_id: large_cs_id,
                    small_repo_id: small_repo.get_repoid(),
                    small_bcs_id: small_cs_id,
                    version_name: Some(mapping_version_name.clone()),
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
                mapping.add_bulk(ctx.clone(), chunk).compat().await?;
                Result::<_, Error>::Ok(len as u64)
            }
        })
        .boxed();

    process_stream_and_wait_for_replication(&ctx, matches, &commit_syncer, s).await?;

    Ok(())
}

async fn process_stream_and_wait_for_replication<'a>(
    ctx: &CoreContext,
    matches: &ArgMatches<'a>,
    commit_syncer: &CommitSyncer<SqlSyncedCommitMapping>,
    mut s: impl Stream<Item = Result<u64>> + std::marker::Unpin,
) -> Result<(), Error> {
    let small_repo = commit_syncer.get_small_repo();
    let large_repo = commit_syncer.get_large_repo();

    let (_, small_repo_config) =
        args::get_config_by_repoid(ctx.fb, matches, small_repo.get_repoid())?;
    let (_, large_repo_config) =
        args::get_config_by_repoid(ctx.fb, matches, large_repo.get_repoid())?;
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

async fn get_commit_syncer(
    ctx: &CoreContext,
    matches: &ArgMatches<'_>,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>> {
    let target_repo_id = args::get_target_repo_id(ctx.fb, &matches)?;
    let config_store = args::maybe_init_config_store(ctx.fb, ctx.logger(), &matches)
        .ok_or_else(|| format_err!("Failed initializing ConfigStore"))?;
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(ctx.logger(), &config_store)?;
    let commit_syncer_args =
        create_commit_syncer_args_from_matches(ctx.fb, ctx.logger(), &matches).await?;
    let commit_sync_config =
        live_commit_sync_config.get_current_commit_sync_config(&ctx, target_repo_id)?;
    commit_syncer_args
        .try_into_commit_syncer(&commit_sync_config, Arc::new(live_commit_sync_config))
}

fn get_version(matches: &ArgMatches<'_>) -> Result<CommitSyncConfigVersion> {
    Ok(CommitSyncConfigVersion(
        matches
            .value_of(VERSION)
            .ok_or_else(|| format_err!("{} not set", VERSION))?
            .to_string(),
    ))
}

fn get_and_verify_repo_config<'a>(
    fb: FacebookInit,
    matches: &ArgMatches<'a>,
) -> Result<RepoConfig> {
    args::get_config(fb, &matches).and_then(|(repo_name, repo_config)| {
        let repo_id = repo_config.repoid;
        repo_config
            .commit_sync_config
            .as_ref()
            .ok_or_else(|| format_err!("no sync config provided for {}", repo_name))
            .map(|commit_sync_config| commit_sync_config.large_repo_id)
            .and_then(move |large_repo_id| {
                if repo_id != large_repo_id {
                    Err(format_err!(
                        "repo must be a large repo in commit sync config"
                    ))
                } else {
                    Ok(repo_config)
                }
            })
    })
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = setup_app();
    let matches = app.get_matches();
    args::init_cachelib(fb, &matches, None);
    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let subcommand_future = async {
        match matches.subcommand() {
            (MOVE, Some(sub_m)) => {
                let repo_config = get_and_verify_repo_config(fb, &matches)?;
                run_move(ctx, &matches, sub_m, repo_config).await
            }
            (MERGE, Some(sub_m)) => run_merge(ctx, &matches, sub_m).await,
            (SYNC_DIAMOND_MERGE, Some(sub_m)) => run_sync_diamond_merge(ctx, &matches, sub_m).await,
            (PRE_MERGE_DELETE, Some(sub_m)) => run_pre_merge_delete(ctx, &matches, sub_m).await,
            (BONSAI_MERGE, Some(sub_m)) => run_bonsai_merge(ctx, &matches, sub_m).await,
            (GRADUAL_MERGE, Some(sub_m)) => run_gradual_merge(ctx, &matches, sub_m).await,
            (GRADUAL_MERGE_PROGRESS, Some(sub_m)) => {
                run_gradual_merge_progress(ctx, &matches, sub_m).await
            }
            (MANUAL_COMMIT_SYNC, Some(sub_m)) => run_manual_commit_sync(ctx, &matches, sub_m).await,
            (CATCHUP_DELETE_HEAD, Some(sub_m)) => {
                run_catchup_delete_head(ctx, &matches, sub_m).await
            }
            (CATCHUP_VALIDATE_COMMAND, Some(sub_m)) => {
                run_catchup_validate(ctx, &matches, sub_m).await
            }
            (MARK_NOT_SYNCED_COMMAND, Some(sub_m)) => {
                run_mark_not_synced(ctx, &matches, sub_m).await
            }
            (RUN_MOVER, Some(sub_m)) => run_mover(ctx, &matches, sub_m).await,
            (BACKFILL_NOOP_MAPPING, Some(sub_m)) => {
                run_backfill_noop_mapping(ctx, &matches, sub_m).await
            }
            _ => bail!("oh no, wrong arguments provided!"),
        }
    };

    helpers::block_execute(
        subcommand_future,
        fb,
        "megarepotool",
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
