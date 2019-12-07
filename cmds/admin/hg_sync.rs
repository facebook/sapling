/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use bookmarks::{BookmarkUpdateReason, Bookmarks, Freshness};
use clap::ArgMatches;
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use dbbookmarks::SqlBookmarks;
use failure_ext::FutureFailureErrorExt;
use fbinit::FacebookInit;
use futures::future::{self, ok};
use futures::prelude::*;
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use mononoke_hg_sync_job_helper_lib::save_bundle_to_file;
use mononoke_types::RepositoryId;
use mutable_counters::{MutableCounters, SqlMutableCounters};
use slog::{info, Logger};

use crate::cmdargs::{
    HG_SYNC_FETCH_BUNDLE, HG_SYNC_LAST_PROCESSED, HG_SYNC_REMAINS, HG_SYNC_SHOW, HG_SYNC_VERIFY,
};
use crate::common::{format_bookmark_log_entry, LATEST_REPLAYED_REQUEST_KEY};
use crate::error::SubcommandError;

pub fn subcommand_process_hg_sync(
    fb: FacebookInit,
    sub_m: &ArgMatches<'_>,
    matches: &ArgMatches<'_>,
    logger: Logger,
) -> BoxFuture<(), SubcommandError> {
    let repo_id =
        try_boxfuture!(args::get_repo_id(&matches).map_err(|e| SubcommandError::Error(e)));

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let mutable_counters = args::open_sql::<SqlMutableCounters>(&matches)
        .context("While opening SqlMutableCounters")
        .from_err();

    let bookmarks = args::open_sql::<SqlBookmarks>(&matches)
        .context("While opening SqlBookmarks")
        .from_err();

    match sub_m.subcommand() {
        (HG_SYNC_LAST_PROCESSED, Some(sub_m)) => match (
            sub_m.value_of("set"),
            sub_m.is_present("skip-blobimport"),
            sub_m.is_present("dry-run"),
        ) {
            (Some(..), true, ..) => {
                future::err(Error::msg("cannot pass both --set and --skip-blobimport"))
                    .from_err()
                    .boxify()
            }
            (.., false, true) => future::err(Error::msg(
                "--dry-run is meaningless without --skip-blobimport",
            ))
            .from_err()
            .boxify(),
            (Some(new_value), false, false) => {
                let new_value = i64::from_str_radix(new_value, 10).unwrap();
                mutable_counters
                    .and_then(move |mutable_counters| {
                        mutable_counters
                            .set_counter(
                                ctx.clone(),
                                repo_id,
                                LATEST_REPLAYED_REQUEST_KEY,
                                new_value,
                                None,
                            )
                            .map({
                                cloned!(repo_id, logger);
                                move |_| {
                                    info!(logger, "Counter for {:?} set to {}", repo_id, new_value);
                                    ()
                                }
                            })
                            .map_err({
                                cloned!(repo_id, logger);
                                move |e| {
                                    info!(
                                        logger,
                                        "Failed to set counter for {:?} set to {}",
                                        repo_id,
                                        new_value
                                    );
                                    e
                                }
                            })
                    })
                    .from_err()
                    .boxify()
            }
            (None, skip, dry_run) => mutable_counters
                .join(bookmarks)
                .and_then(move |(mutable_counters, bookmarks)| {
                    mutable_counters
                        .get_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY)
                        .and_then(move |maybe_counter| {
                            match maybe_counter {
                                None => info!(logger, "No counter found for {:?}", repo_id), //println!("No counter found for {:?}", repo_id),
                                Some(counter) => {
                                    info!(logger, "Counter for {:?} has value {}", repo_id, counter)
                                }
                            };

                            match (skip, maybe_counter) {
                                (false, ..) => {
                                    // We just want to log the current counter: we're done.
                                    ok(()).boxify()
                                }
                                (true, None) => {
                                    // We'd like to skip, but we didn't find the current counter!
                                    future::err(Error::msg("cannot proceed without a counter"))
                                        .boxify()
                                }
                                (true, Some(counter)) => bookmarks
                                    .skip_over_bookmark_log_entries_with_reason(
                                        ctx.clone(),
                                        counter as u64,
                                        repo_id,
                                        BookmarkUpdateReason::Blobimport,
                                    )
                                    .and_then({
                                        cloned!(ctx, repo_id);
                                        move |maybe_new_counter| match (maybe_new_counter, dry_run)
                                        {
                                            (Some(new_counter), true) => {
                                                info!(
                                                    logger,
                                                    "Counter for {:?} would be updated to {}",
                                                    repo_id,
                                                    new_counter
                                                );
                                                future::ok(()).boxify()
                                            }
                                            (Some(new_counter), false) => mutable_counters
                                                .set_counter(
                                                    ctx.clone(),
                                                    repo_id,
                                                    LATEST_REPLAYED_REQUEST_KEY,
                                                    new_counter as i64,
                                                    Some(counter),
                                                )
                                                .and_then(move |success| match success {
                                                    true => {
                                                        info!(
                                                            logger,
                                                            "Counter for {:?} was updated to {}",
                                                            repo_id,
                                                            new_counter
                                                        );
                                                        future::ok(())
                                                    }
                                                    false => {
                                                        future::err(Error::msg("update conflicted"))
                                                    }
                                                })
                                                .boxify(),
                                            (None, ..) => future::err(Error::msg(
                                                "no valid counter position to skip ahead to",
                                            ))
                                            .boxify(),
                                        }
                                    })
                                    .boxify(),
                            }
                        })
                })
                .from_err()
                .boxify(),
        },
        (HG_SYNC_REMAINS, Some(sub_m)) => {
            let quiet = sub_m.is_present("quiet");
            let without_blobimport = sub_m.is_present("without-blobimport");
            mutable_counters
                .join(bookmarks)
                .and_then(move |(mutable_counters, bookmarks)| {
                    mutable_counters
                        .get_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY)
                        .map(|maybe_counter| {
                            // yes, technically if the sync hasn't started yet
                            // and there exists a counter #0, we want return the
                            // correct value, but it's ok, since (a) there won't
                            // be a counter #0 and (b) this is just an advisory data
                            maybe_counter.unwrap_or(0)
                        })
                        .and_then({
                            cloned!(ctx, repo_id, without_blobimport);
                            move |counter| {
                                let counter = counter as u64;

                                let exclude_reason = match without_blobimport {
                                    true => Some(BookmarkUpdateReason::Blobimport),
                                    false => None,
                                };

                                bookmarks.count_further_bookmark_log_entries(
                                    ctx,
                                    counter,
                                    repo_id,
                                    exclude_reason,
                                )
                            }
                        })
                        .map({
                            cloned!(logger, repo_id);
                            move |remaining| {
                                if quiet {
                                    println!("{}", remaining);
                                } else {
                                    let name = match without_blobimport {
                                        true => "non-blobimport bundles",
                                        false => "bundles",
                                    };

                                    info!(
                                        logger,
                                        "Remaining {} to replay in {:?}: {}",
                                        name,
                                        repo_id,
                                        remaining
                                    );
                                }
                            }
                        })
                        .map_err({
                            cloned!(logger, repo_id);
                            move |e| {
                                info!(
                                    logger,
                                    "Failed to fetch remaining bundles to replay for {:?}", repo_id
                                );
                                e
                            }
                        })
                })
                .from_err()
                .boxify()
        }
        (HG_SYNC_SHOW, Some(sub_m)) => {
            let limit = args::get_u64(sub_m, "limit", 10);
            args::init_cachelib(fb, &matches);
            let repo = args::open_repo(fb, &logger, &matches);

            repo.join3(mutable_counters, bookmarks)
                .and_then(move |(repo, mutable_counters, bookmarks)| {
                    mutable_counters
                        .get_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY)
                        .map(|maybe_counter| {
                            // yes, technically if the sync hasn't started yet
                            // and there exists a counter #0, we want return the
                            // correct value, but it's ok, since (a) there won't
                            // be a counter #0 and (b) this is just an advisory data
                            maybe_counter.unwrap_or(0)
                        })
                        .map({
                            cloned!(ctx);
                            move |id| {
                                bookmarks.read_next_bookmark_log_entries(
                                    ctx.clone(),
                                    id as u64,
                                    repo_id,
                                    limit,
                                    Freshness::MostRecent,
                                )
                            }
                        })
                        .flatten_stream()
                        .and_then({
                            cloned!(ctx);
                            move |entry| {
                                let bundle_id = entry.id;
                                match entry.to_changeset_id {
                                    Some(bcs_id) => repo
                                        .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                                        .map(|hg_cs_id| format!("{}", hg_cs_id))
                                        .left_future(),
                                    None => future::ok("DELETED".to_string()).right_future(),
                                }
                                .map(move |hg_cs_id| {
                                    format_bookmark_log_entry(
                                        false, /* json_flag */
                                        hg_cs_id,
                                        entry.reason,
                                        entry.timestamp,
                                        "hg",
                                        entry.bookmark_name,
                                        Some(bundle_id),
                                    )
                                })
                            }
                        })
                        .for_each(|s| {
                            println!("{}", s);
                            Ok(())
                        })
                })
                .from_err()
                .boxify()
        }
        (HG_SYNC_FETCH_BUNDLE, Some(sub_m)) => {
            args::init_cachelib(fb, &matches);
            let repo_fut = args::open_repo(fb, &logger, &matches);
            let id = args::get_u64_opt(sub_m, "id");
            let id = try_boxfuture!(id.ok_or(Error::msg("--id is not specified")));
            if id == 0 {
                return future::err(Error::msg("--id has to be greater than 0"))
                    .from_err()
                    .boxify();
            }

            let output_file = try_boxfuture!(sub_m
                .value_of("output-file")
                .ok_or(Error::msg("--output-file is not specified"))
                .map(std::path::PathBuf::from));

            bookmarks
                .and_then(move |bookmarks| {
                    bookmarks
                        .read_next_bookmark_log_entries(
                            ctx.clone(),
                            id - 1,
                            repo_id,
                            1,
                            Freshness::MostRecent,
                        )
                        .into_future()
                        .map(|(entry, _)| entry)
                        .map_err(|(err, _)| err)
                        .and_then(move |maybe_log_entry| {
                            let log_entry = try_boxfuture!(
                                maybe_log_entry.ok_or(Error::msg("no log entries found"))
                            );
                            if log_entry.id != id as i64 {
                                return future::err(Error::msg("no entry with specified id found"))
                                    .boxify();
                            }
                            let bundle_replay_data = try_boxfuture!(log_entry
                                .reason
                                .get_bundle_replay_data()
                                .ok_or(Error::msg("no bundle found")));
                            let bundle_handle = bundle_replay_data.bundle_handle.clone();

                            repo_fut
                                .and_then(move |repo| {
                                    save_bundle_to_file(
                                        ctx,
                                        repo.get_blobstore(),
                                        &bundle_handle,
                                        output_file,
                                        true, /* create */
                                    )
                                })
                                .boxify()
                        })
                })
                .from_err()
                .boxify()
        }
        (HG_SYNC_VERIFY, Some(..)) => mutable_counters
            .join(bookmarks)
            .and_then({
                cloned!(repo_id);
                move |(mutable_counters, bookmarks)| {
                    process_hg_sync_verify(ctx, repo_id, mutable_counters, bookmarks, logger)
                }
            })
            .from_err()
            .boxify(),
        _ => Err(SubcommandError::InvalidArgs).into_future().boxify(),
    }
}

fn process_hg_sync_verify(
    ctx: CoreContext,
    repo_id: RepositoryId,
    mutable_counters: SqlMutableCounters,
    bookmarks: SqlBookmarks,
    logger: Logger,
) -> impl Future<Item = (), Error = Error> {
    mutable_counters
        .get_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY)
        .map(|maybe_counter| maybe_counter.unwrap_or(0)) // See rationale under HG_SYNC_REMAINS
        .and_then({
            cloned!(ctx, repo_id);
            move |counter| {
                bookmarks.count_further_bookmark_log_entries_by_reason(
                    ctx,
                    counter as u64,
                    repo_id
                )
            }
        })
        .map({
            cloned!(repo_id, logger);
            move |counts| {
                let (
                    blobimports,
                    others
                ): (
                    Vec<(BookmarkUpdateReason, u64)>,
                    Vec<(BookmarkUpdateReason, u64)>
                ) = counts
                    .into_iter()
                    .partition(|(reason, _)| match reason {
                        BookmarkUpdateReason::Blobimport => true,
                        _ => false,
                    });

                let blobimports: u64 = blobimports
                    .into_iter()
                    .fold(0, |acc, (_, count)| acc + count);

                let others: u64 = others
                    .into_iter()
                    .fold(0, |acc, (_, count)| acc + count);

                match (blobimports > 0, others > 0) {
                    (true, true) => {
                        info!(
                            logger,
                            "Remaining bundles to replay in {:?} are not consistent: found {} blobimports and {} non-blobimports",
                            repo_id,
                            blobimports,
                            others
                        );
                    }
                    (true, false) => {
                        info!(
                            logger,
                            "All remaining bundles in {:?} are blobimports (found {})",
                            repo_id,
                            blobimports,
                        );
                    }
                    (false, true) => {
                        info!(
                            logger,
                            "All remaining bundles in {:?} are non-blobimports (found {})",
                            repo_id,
                            others,
                        );
                    }
                    (false, false) =>  {
                        info!(logger, "No replay data found in {:?}", repo_id);
                    }
                };

                ()
            }
        })
}
