/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Context, Error};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::{BookmarkUpdateLog, BookmarkUpdateLogEntry, BookmarkUpdateReason, Freshness};
use clap_old::{App, Arg, ArgMatches, SubCommand};
use cmdlib::args::{self, MononokeMatches};
use context::CoreContext;
use dbbookmarks::SqlBookmarksBuilder;
use fbinit::FacebookInit;
use futures::stream::StreamExt;
use futures::{compat::Future01CompatExt, future};
use mercurial_bundle_replay_data::BundleReplayData;
use mercurial_derived_data::DeriveHgChangeset;
use mononoke_hg_sync_job_helper_lib::save_bundle_to_file;
use mononoke_types::{BonsaiChangeset, ChangesetId, RepositoryId};
use mutable_counters::{MutableCounters, SqlMutableCounters};
use slog::{info, Logger};

use crate::common::{
    format_bookmark_log_entry, print_bonsai_changeset, LATEST_REPLAYED_REQUEST_KEY,
};
use crate::error::SubcommandError;

pub const HG_SYNC_BUNDLE: &str = "hg-sync-bundle";
const HG_SYNC_REMAINS: &str = "remains";
const HG_SYNC_SHOW: &str = "show";
const HG_SYNC_FETCH_BUNDLE: &str = "fetch-bundle";
const HG_SYNC_LAST_PROCESSED: &str = "last-processed";
const HG_SYNC_VERIFY: &str = "verify";
const HG_SYNC_INSPECT: &str = "inspect";

const ARG_SET: &str = "set";
const ARG_SKIP_BLOBIMPORT: &str = "skip-blobimport";
const ARG_DRY_RUN: &str = "dry-run";

const ARG_QUIET: &str = "quiet";
const ARG_WITHOUT_BLOBIMPORT: &str = "without-blobimport";

const ARG_ID: &str = "id";
const ARG_OUTPUT_FILE: &str = "output-file";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(HG_SYNC_BUNDLE)
        .about("things related to mononoke-hg-sync counters")
        .subcommand(
            SubCommand::with_name(HG_SYNC_LAST_PROCESSED)
                .about("inspect/change mononoke-hg sync last processed counter")
                .arg(
                    Arg::with_name(ARG_SET)
                        .long(ARG_SET)
                        .required(false)
                        .takes_value(true)
                        .help("set the value of the latest processed mononoke-hg-sync counter"),
                )
                .arg(
                    Arg::with_name(ARG_SKIP_BLOBIMPORT)
                        .long(ARG_SKIP_BLOBIMPORT)
                        .required(false)
                        .help("skip to the next non-blobimport entry in mononoke-hg-sync counter"),
                )
                .arg(
                    Arg::with_name(ARG_DRY_RUN)
                        .long(ARG_DRY_RUN)
                        .required(false)
                        .help("don't make changes, only show what would have been done"),
                ),
        )
        .subcommand(
            SubCommand::with_name(HG_SYNC_REMAINS)
                .about("get the value of the last mononoke-hg-sync counter to be processed")
                .arg(
                    Arg::with_name(ARG_QUIET)
                        .long(ARG_QUIET)
                        .required(false)
                        .takes_value(false)
                        .help("only print the number if present"),
                )
                .arg(
                    Arg::with_name(ARG_WITHOUT_BLOBIMPORT)
                        .long(ARG_WITHOUT_BLOBIMPORT)
                        .required(false)
                        .takes_value(false)
                        .help("exclude blobimport entries from the count"),
                ),
        )
        .subcommand(
            SubCommand::with_name(HG_SYNC_SHOW)
                .about("show hg hashes of yet to be replayed bundles")
                .arg(
                    Arg::with_name("limit")
                        .long("limit")
                        .required(false)
                        .takes_value(true)
                        .help("how many bundles to show"),
                ),
        )
        .subcommand(
            SubCommand::with_name(HG_SYNC_FETCH_BUNDLE)
                .about("fetches a bundle by id")
                .arg(
                    Arg::with_name(ARG_ID)
                        .long(ARG_ID)
                        .required(true)
                        .takes_value(true)
                        .help("bookmark log id. If it has associated bundle it will be fetched."),
                )
                .arg(
                    Arg::with_name(ARG_OUTPUT_FILE)
                        .long(ARG_OUTPUT_FILE)
                        .required(true)
                        .takes_value(true)
                        .help("where a bundle will be saved"),
                ),
        )
        .subcommand(
            SubCommand::with_name(HG_SYNC_INSPECT)
                .about("print some information about a log entry")
                .arg(Arg::with_name(ARG_ID).required(true).takes_value(true)),
        )
        .subcommand(
            SubCommand::with_name(HG_SYNC_VERIFY)
                .about("verify the consistency of yet-to-be-processed bookmark log entries"),
        )
}

async fn last_processed(
    sub_m: &ArgMatches<'_>,
    ctx: &CoreContext,
    repo_id: RepositoryId,
    mutable_counters: &SqlMutableCounters,
    bookmarks: &dyn BookmarkUpdateLog,
) -> Result<(), Error> {
    match (
        sub_m.value_of(ARG_SET),
        sub_m.is_present(ARG_SKIP_BLOBIMPORT),
        sub_m.is_present(ARG_DRY_RUN),
    ) {
        (Some(..), true, ..) => Err(format_err!(
            "cannot pass both --{} and --{}",
            ARG_SET,
            ARG_SKIP_BLOBIMPORT
        )),
        (.., false, true) => Err(format_err!(
            "--{} is meaningless without --{}",
            ARG_DRY_RUN,
            ARG_SKIP_BLOBIMPORT
        )),
        (Some(new_value), false, false) => {
            let new_value = i64::from_str_radix(new_value, 10).unwrap();
            mutable_counters
                .set_counter(
                    ctx.clone(),
                    repo_id,
                    LATEST_REPLAYED_REQUEST_KEY,
                    new_value,
                    None,
                )
                .compat()
                .await
                .with_context(|| {
                    format!(
                        "Failed to set counter for {:?} set to {}",
                        repo_id, new_value
                    )
                })?;

            info!(
                ctx.logger(),
                "Counter for {:?} set to {}", repo_id, new_value
            );

            Ok(())
        }
        (None, skip, dry_run) => {
            let maybe_counter = mutable_counters
                .get_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY)
                .compat()
                .await?;

            match maybe_counter {
                None => info!(ctx.logger(), "No counter found for {:?}", repo_id), //println!("No counter found for {:?}", repo_id),
                Some(counter) => info!(
                    ctx.logger(),
                    "Counter for {:?} has value {}", repo_id, counter
                ),
            };

            match (skip, maybe_counter) {
                (false, ..) => {
                    // We just want to log the current counter: we're done.
                    Ok(())
                }
                (true, None) => {
                    // We'd like to skip, but we didn't find the current counter!
                    Err(Error::msg("cannot proceed without a counter"))
                }
                (true, Some(counter)) => {
                    let maybe_new_counter = bookmarks
                        .skip_over_bookmark_log_entries_with_reason(
                            ctx.clone(),
                            counter.try_into()?,
                            BookmarkUpdateReason::Blobimport,
                        )
                        .await?;

                    match (maybe_new_counter, dry_run) {
                        (Some(new_counter), true) => {
                            info!(
                                ctx.logger(),
                                "Counter for {:?} would be updated to {}", repo_id, new_counter
                            );
                            Ok(())
                        }
                        (Some(new_counter), false) => {
                            let success = mutable_counters
                                .set_counter(
                                    ctx.clone(),
                                    repo_id,
                                    LATEST_REPLAYED_REQUEST_KEY,
                                    new_counter as i64,
                                    Some(counter),
                                )
                                .compat()
                                .await?;

                            match success {
                                true => {
                                    info!(
                                        ctx.logger(),
                                        "Counter for {:?} was updated to {}", repo_id, new_counter
                                    );
                                    Ok(())
                                }
                                false => Err(Error::msg("update conflicted")),
                            }
                        }
                        (None, ..) => Err(Error::msg("no valid counter position to skip ahead to")),
                    }
                }
            }
        }
    }
}

async fn remains(
    sub_m: &ArgMatches<'_>,
    ctx: &CoreContext,
    repo_id: RepositoryId,
    mutable_counters: &SqlMutableCounters,
    bookmarks: &dyn BookmarkUpdateLog,
) -> Result<(), Error> {
    let quiet = sub_m.is_present(ARG_QUIET);
    let without_blobimport = sub_m.is_present(ARG_WITHOUT_BLOBIMPORT);

    // yes, technically if the sync hasn't started yet
    // and there exists a counter #0, we want return the
    // correct value, but it's ok, since (a) there won't
    // be a counter #0 and (b) this is just an advisory data
    let counter = mutable_counters
        .get_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY)
        .compat()
        .await?
        .unwrap_or(0)
        .try_into()?;

    let exclude_reason = match without_blobimport {
        true => Some(BookmarkUpdateReason::Blobimport),
        false => None,
    };

    let remaining = bookmarks
        .count_further_bookmark_log_entries(ctx.clone(), counter, exclude_reason)
        .await
        .with_context(|| {
            format!(
                "Failed to fetch remaining bundles to replay for {:?}",
                repo_id
            )
        })?;

    if quiet {
        println!("{}", remaining);
    } else {
        let name = match without_blobimport {
            true => "non-blobimport bundles",
            false => "bundles",
        };

        info!(
            ctx.logger(),
            "Remaining {} to replay in {:?}: {}", name, repo_id, remaining
        );
    }

    Ok(())
}

async fn show(
    sub_m: &ArgMatches<'_>,
    ctx: &CoreContext,
    repo: &BlobRepo,
    mutable_counters: &SqlMutableCounters,
    bookmarks: &dyn BookmarkUpdateLog,
) -> Result<(), Error> {
    let limit = args::get_u64(sub_m, "limit", 10);

    // yes, technically if the sync hasn't started yet
    // and there exists a counter #0, we want return the
    // correct value, but it's ok, since (a) there won't
    // be a counter #0 and (b) this is just an advisory data
    let counter = mutable_counters
        .get_counter(ctx.clone(), repo.get_repoid(), LATEST_REPLAYED_REQUEST_KEY)
        .compat()
        .await?
        .unwrap_or(0);

    let mut entries = bookmarks.read_next_bookmark_log_entries(
        ctx.clone(),
        counter.try_into()?,
        limit,
        Freshness::MostRecent,
    );

    while let Some(entry) = entries.next().await {
        let entry = entry?;
        let bundle_id = entry.id;

        let hg_cs_id = match entry.to_changeset_id {
            Some(bcs_id) => repo.derive_hg_changeset(ctx, bcs_id).await?.to_string(),
            None => "DELETED".to_string(),
        };

        let line = format_bookmark_log_entry(
            false, /* json_flag */
            hg_cs_id,
            entry.reason,
            entry.timestamp,
            "hg",
            entry.bookmark_name,
            Some(bundle_id as u64),
        );

        println!("{}", line);
    }

    Ok(())
}

async fn fetch_bundle(
    sub_m: &ArgMatches<'_>,
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmarks: &dyn BookmarkUpdateLog,
) -> Result<(), Error> {
    let id = args::get_i64_opt(&sub_m, ARG_ID)
        .ok_or_else(|| format_err!("--{} is not specified", ARG_ID))?;

    let output_file = sub_m
        .value_of(ARG_OUTPUT_FILE)
        .ok_or_else(|| format_err!("--{} is not specified", ARG_OUTPUT_FILE))?
        .into();

    let log_entry = get_entry_by_id(ctx, bookmarks, id).await?;

    let bundle_replay_data: BundleReplayData = log_entry
        .bundle_replay_data
        .ok_or_else(|| Error::msg("no bundle found"))?
        .try_into()?;

    save_bundle_to_file(
        &ctx,
        repo.blobstore(),
        bundle_replay_data.bundle2_id,
        output_file,
        true, /* create */
    )
    .await?;

    Ok(())
}

async fn verify(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    mutable_counters: &SqlMutableCounters,
    bookmarks: &dyn BookmarkUpdateLog,
) -> Result<(), Error> {
    let counter = mutable_counters
        .get_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY)
        .compat()
        .await?
        .unwrap_or(0) // See rationale in remains()
        .try_into()?;

    let counts = bookmarks
        .count_further_bookmark_log_entries_by_reason(ctx.clone(), counter)
        .await?;

    let (blobimports, others): (
        Vec<(BookmarkUpdateReason, u64)>,
        Vec<(BookmarkUpdateReason, u64)>,
    ) = counts.into_iter().partition(|(reason, _)| match reason {
        BookmarkUpdateReason::Blobimport => true,
        _ => false,
    });

    let blobimports: u64 = blobimports
        .into_iter()
        .fold(0, |acc, (_, count)| acc + count);

    let others: u64 = others.into_iter().fold(0, |acc, (_, count)| acc + count);

    match (blobimports > 0, others > 0) {
        (true, true) => {
            info!(
                ctx.logger(),
                "Remaining bundles to replay in {:?} are not consistent: found {} blobimports and {} non-blobimports",
                repo_id,
                blobimports,
                others
            );
        }
        (true, false) => {
            info!(
                ctx.logger(),
                "All remaining bundles in {:?} are blobimports (found {})", repo_id, blobimports,
            );
        }
        (false, true) => {
            info!(
                ctx.logger(),
                "All remaining bundles in {:?} are non-blobimports (found {})", repo_id, others,
            );
        }
        (false, false) => {
            info!(ctx.logger(), "No replay data found in {:?}", repo_id);
        }
    };

    Ok(())
}

async fn inspect(
    sub_m: &ArgMatches<'_>,
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmarks: &dyn BookmarkUpdateLog,
) -> Result<(), Error> {
    async fn load_opt(
        ctx: &CoreContext,
        repo: &BlobRepo,
        cs_id: &Option<ChangesetId>,
    ) -> Result<Option<BonsaiChangeset>, Error> {
        let maybe_bcs = match cs_id {
            Some(ref cs_id) => {
                let bcs = cs_id.load(ctx, repo.blobstore()).await?;
                Some(bcs)
            }
            None => None,
        };

        Ok(maybe_bcs)
    }

    let id = args::get_i64_opt(&sub_m, ARG_ID)
        .ok_or_else(|| format_err!("--{} is not specified", ARG_ID))?;

    let log_entry = get_entry_by_id(ctx, bookmarks, id).await?;

    println!("Bookmark: {}", log_entry.bookmark_name);

    let (from_bcs, to_bcs) = future::try_join(
        load_opt(ctx, repo, &log_entry.from_changeset_id),
        load_opt(ctx, repo, &log_entry.to_changeset_id),
    )
    .await?;

    match from_bcs {
        Some(bcs) => {
            println!("=== From ===");
            print_bonsai_changeset(&bcs);
        }
        None => {
            info!(ctx.logger(), "Log entry is a bookmark creation.");
        }
    }

    match to_bcs {
        Some(bcs) => {
            println!("=== To ===");
            print_bonsai_changeset(&bcs);
        }
        None => {
            info!(ctx.logger(), "Log entry is a bookmark deletion.");
        }
    }

    Ok(())
}

async fn get_entry_by_id(
    ctx: &CoreContext,
    bookmarks: &dyn BookmarkUpdateLog,
    id: i64,
) -> Result<BookmarkUpdateLogEntry, Error> {
    let log_entry = bookmarks
        .read_next_bookmark_log_entries(ctx.clone(), (id - 1).try_into()?, 1, Freshness::MostRecent)
        .next()
        .await
        .ok_or_else(|| Error::msg("no log entries found"))??;

    if log_entry.id != id {
        return Err(format_err!("no entry with id {} found", id));
    }

    Ok(log_entry)
}

pub async fn subcommand_process_hg_sync<'a>(
    fb: FacebookInit,
    sub_m: &'a ArgMatches<'_>,
    matches: &'a MononokeMatches<'_>,
    logger: Logger,
) -> Result<(), SubcommandError> {
    let config_store = matches.config_store();

    let repo_id = args::get_repo_id(config_store, &matches)?;

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let mutable_counters = args::open_sql::<SqlMutableCounters>(fb, config_store, &matches)
        .context("While opening SqlMutableCounters")?;

    let bookmarks = args::open_sql::<SqlBookmarksBuilder>(fb, config_store, &matches)
        .context("While opening SqlBookmarks")?
        .with_repo_id(repo_id);

    let res = match sub_m.subcommand() {
        (HG_SYNC_LAST_PROCESSED, Some(sub_m)) => {
            last_processed(sub_m, &ctx, repo_id, &mutable_counters, &bookmarks).await?
        }
        (HG_SYNC_REMAINS, Some(sub_m)) => {
            remains(sub_m, &ctx, repo_id, &mutable_counters, &bookmarks).await?
        }
        (HG_SYNC_SHOW, Some(sub_m)) => {
            let repo = args::open_repo(fb, ctx.logger(), &matches).await?;
            show(sub_m, &ctx, &repo, &mutable_counters, &bookmarks).await?
        }
        (HG_SYNC_FETCH_BUNDLE, Some(sub_m)) => {
            let repo = args::open_repo(fb, ctx.logger(), &matches).await?;
            fetch_bundle(sub_m, &ctx, &repo, &bookmarks).await?
        }
        (HG_SYNC_INSPECT, Some(sub_m)) => {
            let repo = args::open_repo(fb, ctx.logger(), &matches).await?;
            inspect(sub_m, &ctx, &repo, &bookmarks).await?
        }
        (HG_SYNC_VERIFY, Some(..)) => {
            verify(&ctx, repo_id, &mutable_counters, &bookmarks).await?;
        }
        _ => return Err(SubcommandError::InvalidArgs),
    };

    Ok(res)
}
