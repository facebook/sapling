/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(optin_builtin_traits)]
#![deny(warnings)]

/// Mononoke Cross Repo validator job
///
/// This is a special job used to validate that cross-repo sync,
/// produced correct results
use anyhow::{format_err, Context, Error, Result};
use blobrepo::BlobRepo;
use bookmarks::{BookmarkUpdateLog, BookmarkUpdateLogEntry, Freshness};
use clap::{App, ArgMatches};
use cmdlib::{
    args,
    helpers::{block_execute, open_sql_with_config_and_mysql_options},
    monitoring::AliveService,
};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures::future;
use futures::stream::{self, Stream, StreamExt, TryStreamExt};
use mutable_counters::MutableCounters;
use mutable_counters::SqlMutableCounters;
use scuba_ext::ScubaSampleBuilder;

mod cli;
mod reporting;
mod setup;
mod tail;
mod validation;

use crate::cli::{create_app, ARG_ONCE, ARG_TAIL};
use crate::setup::{format_counter, get_entry_id, get_start_id, get_validation_helpers};
use crate::tail::{tail_entries, QueueSize};
use crate::validation::{
    get_entry_with_small_repo_mapings, prepare_entry, unfold_bookmarks_update_log_entry,
    validate_entry, EntryCommitId, ValidationHelpers,
};

const SERVICE_NAME: &str = "mononoke_x_repo_commit_validator";

fn validate_stream<'a>(
    ctx: &'a CoreContext,
    validation_helpers: &'a ValidationHelpers,
    entries: impl Stream<Item = Result<(BookmarkUpdateLogEntry, QueueSize), Error>> + 'a,
) -> impl Stream<Item = Result<EntryCommitId, Error>> + 'a {
    entries
        .then(move |bookmarks_update_log_entry_res| async move {
            unfold_bookmarks_update_log_entry(
                ctx,
                bookmarks_update_log_entry_res?,
                validation_helpers,
            )
            .await
        })
        .try_flatten()
        .then(move |res_entry| async move {
            get_entry_with_small_repo_mapings(ctx, res_entry?, validation_helpers).await
        })
        .filter_map(|maybe_entry_res| future::ready(maybe_entry_res.transpose()))
        .then(move |res_entry| async move {
            prepare_entry(ctx, res_entry?, validation_helpers).await
        })
        .map(|res_of_option| res_of_option.transpose())
        .filter_map(future::ready)
        .then(move |prepared_entry| async move {
            let prepared_entry = prepared_entry?;
            let entry_id = prepared_entry.entry_id.clone();

            validate_entry(ctx, prepared_entry, validation_helpers).await?;

            Ok(entry_id)
        })
}

async fn run_in_tailing_mode<T: MutableCounters>(
    ctx: &CoreContext,
    blobrepo: BlobRepo,
    validation_helpers: ValidationHelpers,
    start_id: u64,
    scuba_sample: ScubaSampleBuilder,
    mutable_counters: &T,
) -> Result<(), Error> {
    let counter_name = format_counter();
    let stream_of_entries = tail_entries(
        ctx.clone(),
        start_id,
        blobrepo.get_repoid(),
        blobrepo
            .attribute_expected::<dyn BookmarkUpdateLog>()
            .clone(),
        scuba_sample,
    );

    validate_stream(&ctx, &validation_helpers, stream_of_entries)
        .then(
            |validated_entry_id_res: Result<EntryCommitId, Error>| async {
                let entry_id = validated_entry_id_res?;
                if entry_id.last_commit_for_bookmark_move() {
                    let id = entry_id.bookmarks_update_log_entry_id;
                    mutable_counters
                        .set_counter(ctx.clone(), blobrepo.get_repoid(), &counter_name, id, None)
                        .compat()
                        .await?;
                }

                Result::<_, Error>::Ok(())
            },
        )
        .try_for_each(|_| future::ready(Ok(())))
        .await
}

async fn run_in_once_mode(
    ctx: &CoreContext,
    blobrepo: BlobRepo,
    validation_helpers: ValidationHelpers,
    entry_id: u64,
) -> Result<(), Error> {
    let bookmark_update_log = blobrepo.attribute_expected::<dyn BookmarkUpdateLog>();
    let entries: Vec<Result<(BookmarkUpdateLogEntry, QueueSize), Error>> = bookmark_update_log
        .read_next_bookmark_log_entries(
            ctx.clone(),
            entry_id - 1,
            1, /* limit */
            Freshness::MaybeStale,
        )
        .map_ok(|entry: BookmarkUpdateLogEntry| (entry, QueueSize(0)))
        .collect()
        .await;

    if entries.len() == 0 {
        return Err(format_err!(
            "No entries for {} with id >{}",
            blobrepo.get_repoid(),
            entry_id - 1
        ));
    }

    let stream_of_entries = stream::iter(entries);
    validate_stream(&ctx, &validation_helpers, stream_of_entries)
        .try_for_each(|_| future::ready(Ok(())))
        .await
}

async fn run(
    fb: FacebookInit,
    ctx: CoreContext,
    matches: ArgMatches<'static>,
) -> Result<(), Error> {
    let repo_id = args::get_repo_id(fb, &matches)?;
    let (_, repo_config) = args::get_config_by_repoid(fb, &matches, repo_id)?;

    let logger = ctx.logger();
    let blobrepo = args::open_repo_with_repo_id(fb, &logger, repo_id, &matches)
        .compat()
        .await
        .with_context(|| format!("While opening the large repo ({})", repo_id))?;
    let mysql_options = args::parse_mysql_options(&matches);
    let readonly_storage = args::parse_readonly_storage(&matches);
    let dbconfig = repo_config.storage_config.metadata.clone();
    let scuba_sample = args::get_scuba_sample_builder(fb, &matches)?;
    let validation_helpers = get_validation_helpers(
        fb,
        ctx.clone(),
        blobrepo.clone(),
        repo_config,
        matches.clone(),
        mysql_options.clone(),
        readonly_storage.clone(),
        scuba_sample.clone(),
    )
    .await
    .context("While instantiating commit syncers")?;

    match matches.subcommand() {
        (ARG_ONCE, Some(sub_m)) => {
            let entry_id = get_entry_id(sub_m.clone())?;
            run_in_once_mode(&ctx, blobrepo, validation_helpers, entry_id).await
        }
        (ARG_TAIL, Some(sub_m)) => {
            let mutable_counters = open_sql_with_config_and_mysql_options::<SqlMutableCounters>(
                fb,
                dbconfig,
                mysql_options,
                readonly_storage,
            )
            .compat()
            .await
            .context("While opening MutableCounters")?;

            let start_id = get_start_id(ctx.clone(), repo_id, &mutable_counters, sub_m.clone())
                .await
                .context("While fetching the start_id")?;

            run_in_tailing_mode(
                &ctx,
                blobrepo,
                validation_helpers,
                start_id,
                scuba_sample,
                &mutable_counters,
            )
            .await
        }
        (_, _) => Err(format_err!("Incorrect command line arguments provided")),
    }
}

fn context_and_matches<'a>(fb: FacebookInit, app: App<'a, '_>) -> (CoreContext, ArgMatches<'a>) {
    let matches = app.get_matches();
    let logger = args::init_logging(fb, &matches);
    args::init_cachelib(fb, &matches, None);
    let ctx = CoreContext::new_with_logger(fb, logger);
    (ctx, matches)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let (ctx, matches) = context_and_matches(fb, create_app());
    block_execute(
        run(fb, ctx.clone(), matches.clone()),
        fb,
        SERVICE_NAME,
        ctx.logger(),
        &matches,
        AliveService,
    )
}
