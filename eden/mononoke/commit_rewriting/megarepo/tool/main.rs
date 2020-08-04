/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![feature(process_exitcode_placeholder)]

use anyhow::{bail, format_err, Error, Result};
use bookmarks::BookmarkName;
use clap::ArgMatches;
use cmdlib::{args, helpers};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    future::{try_join, try_join3},
};
use metaconfig_types::RepoConfig;
use mononoke_types::RepositoryId;
use movers::get_small_to_large_mover;
use skiplist::fetch_skiplist_index;
use slog::info;
use std::collections::BTreeMap;
use std::num::NonZeroU64;
use synced_commit_mapping::SqlSyncedCommitMapping;

mod cli;
mod gradual_merge;
mod merging;
mod sync_diamond_merge;

use crate::cli::{
    cs_args_from_matches, get_delete_commits_cs_args_factory,
    get_gradual_merge_commits_cs_args_factory, setup_app, BONSAI_MERGE, BONSAI_MERGE_P1,
    BONSAI_MERGE_P2, CHANGESET, CHUNKING_HINT_FILE, COMMIT_BOOKMARK, COMMIT_HASH, DRY_RUN,
    EVEN_CHUNK_SIZE, FIRST_PARENT, GRADUAL_MERGE, LAST_DELETION_COMMIT, LIMIT,
    MAX_NUM_OF_MOVES_IN_COMMIT, MERGE, MOVE, ORIGIN_REPO, PRE_DELETION_COMMIT, PRE_MERGE_DELETE,
    SECOND_PARENT, SYNC_DIAMOND_MERGE,
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

    sync_diamond_merge::do_sync_diamond_merge(
        ctx,
        source_repo,
        target_repo,
        source_merge_cs_id,
        mapping,
        source_repo_config,
        bookmark,
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

    let pmd = create_pre_merge_delete(&ctx, &repo, parent_bcs_id, chunker, delete_cs_args_factory)
        .await?;

    let PreMergeDelete { mut delete_commits } = pmd;

    info!(ctx.logger(), "Listing deletion commits in top-to-bottom order (first commit is a descendant of the last)");
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
