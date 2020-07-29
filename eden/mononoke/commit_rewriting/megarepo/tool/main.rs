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
use slog::info;
use std::num::NonZeroU64;
use synced_commit_mapping::SqlSyncedCommitMapping;

mod cli;
mod merging;
mod sync_diamond_merge;

use crate::cli::{
    cs_args_from_matches, setup_app, CHANGESET, COMMIT_HASH, FIRST_PARENT,
    MAX_NUM_OF_MOVES_IN_COMMIT, MERGE, MOVE, ORIGIN_REPO, SECOND_PARENT, SYNC_DIAMOND_MERGE,
};
use crate::merging::perform_merge;
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
