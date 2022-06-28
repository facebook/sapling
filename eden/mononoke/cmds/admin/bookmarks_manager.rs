/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use blobrepo_hg::BlobRepoHg;
use bookmarks::Freshness;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use cloned::cloned;
use context::CoreContext;
use futures::TryStreamExt;
use humantime::parse_duration;
use mercurial_derived_data::DeriveHgChangeset;
use mononoke_types::Timestamp;
use repo_blobstore::RepoBlobstoreRef;
use serde_json::json;
use serde_json::to_string_pretty;
use slog::info;
use slog::Logger;

use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;

use crate::common::fetch_bonsai_changeset;
use crate::common::format_bookmark_log_entry;
use crate::error::SubcommandError;

pub const BOOKMARKS: &str = "bookmarks";
const SET_CMD: &str = "set";
const GET_CMD: &str = "get";
const LOG_CMD: &str = "log";
const LIST_CMD: &str = "list";
const DEL_CMD: &str = "delete";

const ARG_CHANGESET_TYPE: &str = "changeset-type";
const ARG_LIMIT: &str = "limit";
const ARG_START_TIME: &str = "start-time";
const ARG_END_TIME: &str = "end-time";
const ARG_KIND: &str = "kind";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    let parent_subcommand = SubCommand::with_name(BOOKMARKS);
    let set = SubCommand::with_name(SET_CMD)
        .about(
            "sets a bookmark to a specific hg changeset, if the bookmark does not exist it will
                be created",
        )
        .args_from_usage(
            "<BOOKMARK_NAME>        'bookmark to target'
             <HG_CHANGESET_ID>      'revision to which the bookmark should point to'",
        );

    let get = SubCommand::with_name(GET_CMD)
        .about("gets the changeset of a specific bookmark")
        .args_from_usage(
            r#"
            <BOOKMARK_NAME>        'bookmark to target'
            --json                 'if provided json will be returned'
            "#,
        )
        .arg(
            Arg::with_name(ARG_CHANGESET_TYPE)
                .long(ARG_CHANGESET_TYPE)
                .short("cs")
                .takes_value(true)
                .possible_values(&["bonsai", "hg"])
                .required(false)
                .help("What changeset type to return, either bonsai or hg. Defaults to hg."),
        );

    let log = SubCommand::with_name(LOG_CMD)
        .about("gets the log of changesets for a specific bookmark")
        .args_from_usage(
            r#"
            <BOOKMARK_NAME>        'bookmark to target'
            --json                 'if provided json will be returned'
            "#,
        )
        .arg(
            Arg::with_name(ARG_CHANGESET_TYPE)
                .long(ARG_CHANGESET_TYPE)
                .short("cs")
                .takes_value(true)
                .possible_values(&["bonsai", "hg"])
                .required(false)
                .help("What changeset type to return, either bonsai or hg. Defaults to hg."),
        )
        .arg(
            Arg::with_name(ARG_LIMIT)
                .long(ARG_LIMIT)
                .short("l")
                .takes_value(true)
                .required(false)
                .help("Imposes the limit on number of log records in output."),
        )
        .arg(
            Arg::with_name(ARG_START_TIME)
                .long(ARG_START_TIME)
                .short("s")
                .takes_value(true)
                .required(false)
                .help(
                    "Filter log records by timestamp lower bound. \
                    Takes time difference in free form e.g. 1h, 10m 30s, etc.",
                ),
        )
        .arg(
            Arg::with_name(ARG_END_TIME)
                .long(ARG_END_TIME)
                .short("e")
                .takes_value(true)
                .required(false)
                .requires(ARG_START_TIME)
                .help(
                    "Filter log records by timestamp upper bound. \
                    Takes time difference in free form e.g. 1h, 10m 30s, etc.",
                ),
        );

    let list = SubCommand::with_name(LIST_CMD).about("list bookmarks").arg(
        Arg::with_name(ARG_KIND)
            .long(ARG_KIND)
            .takes_value(true)
            .possible_values(&["publishing"])
            .required(true)
            .help("What set of bookmarks to list"),
    );

    let del = SubCommand::with_name(DEL_CMD)
        .about("delete bookmark")
        .args_from_usage(
            r#"
            <BOOKMARK_NAME>        'bookmark to delete'
            "#,
        );

    parent_subcommand
        .about("set of commands to manipulate bookmarks")
        .subcommand(set)
        .subcommand(get)
        .subcommand(log)
        .subcommand(list)
        .subcommand(del)
}

pub async fn handle_command(
    ctx: CoreContext,
    repo: BlobRepo,
    matches: &ArgMatches<'_>,
    _logger: Logger,
) -> Result<(), SubcommandError> {
    match matches.subcommand() {
        (GET_CMD, Some(sub_m)) => handle_get(sub_m, ctx, repo).await?,
        (SET_CMD, Some(sub_m)) => handle_set(sub_m, ctx, repo).await?,
        (LOG_CMD, Some(sub_m)) => handle_log(sub_m, ctx, repo).await?,
        (LIST_CMD, Some(sub_m)) => handle_list(sub_m, ctx, repo).await?,
        (DEL_CMD, Some(sub_m)) => handle_delete(sub_m, ctx, repo).await?,
        _ => return Err(SubcommandError::InvalidArgs),
    }
    Ok(())
}

fn format_output(json_flag: bool, changeset_id: String, changeset_type: &str) -> String {
    if json_flag {
        let answer = json!({
            "changeset_type": changeset_type,
            "changeset_id": changeset_id
        });
        to_string_pretty(&answer).unwrap()
    } else {
        format!("({}) {}", changeset_type.to_uppercase(), changeset_id)
    }
}

async fn handle_get(args: &ArgMatches<'_>, ctx: CoreContext, repo: BlobRepo) -> Result<(), Error> {
    let bookmark_name = args.value_of("BOOKMARK_NAME").unwrap().to_string();
    let bookmark = BookmarkName::new(bookmark_name).unwrap();
    let changeset_type = args.value_of(ARG_CHANGESET_TYPE).unwrap_or("hg");
    let json_flag = args.is_present("json");

    match changeset_type {
        "hg" => {
            let cs = repo.get_bookmark(ctx, &bookmark).await?;
            let changeset_id_str = cs.expect("bookmark could not be found").to_string();
            let output = format_output(json_flag, changeset_id_str, "hg");
            println!("{}", output);
            Ok(())
        }
        "bonsai" => {
            let bonsai_cs = fetch_bonsai_changeset(
                ctx,
                bookmark.to_string().as_str(),
                &repo,
                repo.repo_blobstore(),
            )
            .await?;
            let changeset_id_str = bonsai_cs.get_changeset_id().to_string();
            let output = format_output(json_flag, changeset_id_str, "bonsai");
            println!("{}", output);
            Ok(())
        }
        _ => Err(format_err!("Unknown changeset-type supplied")),
    }
}

async fn handle_log(args: &ArgMatches<'_>, ctx: CoreContext, repo: BlobRepo) -> Result<(), Error> {
    let bookmark_name = args.value_of("BOOKMARK_NAME").unwrap().to_string();
    let bookmark = BookmarkName::new(bookmark_name).unwrap();
    let changeset_type = args.value_of(ARG_CHANGESET_TYPE).unwrap_or("hg");
    let json_flag = args.is_present("json");
    let output_limit_as_string = args.value_of(ARG_LIMIT).unwrap_or("25");
    let max_rec = match output_limit_as_string.parse::<u32>() {
        Ok(n) => n,
        Err(e) => {
            return Err(format_err!(
                "Bad limit value supplied: \"{}\" - {}",
                output_limit_as_string,
                e
            ));
        }
    };

    let filter_by_ts_range = args.is_present(ARG_START_TIME) || args.is_present(ARG_END_TIME);
    let entries = if filter_by_ts_range {
        let min_ts_diff_ns = parse_duration(args.value_of(ARG_START_TIME).ok_or_else(|| {
            format_err!(
                "{} is required if {} is present",
                ARG_START_TIME,
                ARG_END_TIME
            )
        })?)?
        .as_nanos() as i64;
        let max_ts_diff_ns =
            parse_duration(args.value_of(ARG_END_TIME).unwrap_or("0h"))?.as_nanos() as i64;
        if min_ts_diff_ns < max_ts_diff_ns {
            return Err(format_err!("Start time should be earlier than end time"));
        }
        let current_timestamp_ns = Timestamp::now().timestamp_nanos();
        repo.bookmark_update_log()
            .list_bookmark_log_entries_ts_in_range(
                ctx.clone(),
                bookmark.clone(),
                max_rec,
                Timestamp::from_timestamp_nanos(current_timestamp_ns - min_ts_diff_ns),
                Timestamp::from_timestamp_nanos(current_timestamp_ns - max_ts_diff_ns),
            )
    } else {
        repo.bookmark_update_log().list_bookmark_log_entries(
            ctx.clone(),
            bookmark.clone(),
            max_rec,
            None,
            Freshness::MostRecent,
        )
    };

    match changeset_type {
        "hg" => {
            entries
                .map_ok({
                    cloned!(ctx, repo);
                    move |(entry_id, cs_id, rs, ts)| {
                        cloned!(ctx, repo);
                        async move {
                            match cs_id {
                                Some(cs_id) => {
                                    let cs = repo.derive_hg_changeset(&ctx, cs_id).await?;
                                    Ok((entry_id, Some(cs), rs, ts))
                                }
                                None => Ok((entry_id, None, rs, ts)),
                            }
                        }
                    }
                })
                .try_buffered(100)
                .map_ok(move |rows| {
                    let (entry_id, cs_id, reason, timestamp) = rows;
                    let cs_id_str = match cs_id {
                        None => String::new(),
                        Some(x) => x.to_string(),
                    };
                    let output = format_bookmark_log_entry(
                        json_flag,
                        cs_id_str,
                        reason,
                        timestamp,
                        "hg",
                        bookmark.clone(),
                        Some(entry_id),
                    );
                    println!("{}", output);
                })
                .try_for_each(|_| async { Ok(()) })
                .await
        }
        "bonsai" => {
            entries
                .map_ok(move |rows| {
                    let (entry_id, cs_id, reason, timestamp) = rows;
                    let cs_id_str = match cs_id {
                        None => String::new(),
                        Some(x) => x.to_string(),
                    };
                    let output = format_bookmark_log_entry(
                        json_flag,
                        cs_id_str,
                        reason,
                        timestamp,
                        "bonsai",
                        bookmark.clone(),
                        Some(entry_id),
                    );
                    println!("{}", output);
                })
                .try_for_each(|_| async { Ok(()) })
                .await
        }
        _ => Err(format_err!("Unknown changeset-type supplied")),
    }
}

async fn handle_list(args: &ArgMatches<'_>, ctx: CoreContext, repo: BlobRepo) -> Result<(), Error> {
    match args.value_of(ARG_KIND) {
        Some("publishing") => {
            repo.get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
                .try_for_each_concurrent(100, {
                    cloned!(repo, ctx);
                    move |(bookmark, bonsai_cs_id)| {
                        cloned!(ctx, repo);
                        async move {
                            let hg_cs_id = repo.derive_hg_changeset(&ctx, bonsai_cs_id).await?;
                            println!("{}\t{}\t{}", bookmark.into_name(), bonsai_cs_id, hg_cs_id);
                            Ok(())
                        }
                    }
                })
                .await
        }
        kind => Err(format_err!("Invalid kind {:?}", kind)),
    }
}

async fn handle_set(args: &ArgMatches<'_>, ctx: CoreContext, repo: BlobRepo) -> Result<(), Error> {
    let bookmark_name = args.value_of("BOOKMARK_NAME").unwrap().to_string();
    let rev = args.value_of("HG_CHANGESET_ID").unwrap().to_string();
    let bookmark = BookmarkName::new(bookmark_name).unwrap();
    let new_bcs = fetch_bonsai_changeset(ctx.clone(), &rev, &repo, repo.repo_blobstore()).await?;
    let maybe_old_bcs_id = repo.get_bonsai_bookmark(ctx.clone(), &bookmark).await?;
    info!(
        ctx.logger(),
        "Current position of {:?} is {:?}", bookmark, maybe_old_bcs_id
    );
    let mut transaction = repo.update_bookmark_transaction(ctx);
    match maybe_old_bcs_id {
        Some(old_bcs_id) => {
            transaction.update(
                &bookmark,
                new_bcs.get_changeset_id(),
                old_bcs_id,
                BookmarkUpdateReason::ManualMove,
            )?;
        }
        None => {
            transaction.create(
                &bookmark,
                new_bcs.get_changeset_id(),
                BookmarkUpdateReason::ManualMove,
            )?;
        }
    }
    transaction.commit().await?;
    Ok(())
}

async fn handle_delete(
    args: &ArgMatches<'_>,
    ctx: CoreContext,
    repo: BlobRepo,
) -> Result<(), Error> {
    let bookmark_name = args.value_of("BOOKMARK_NAME").unwrap().to_string();
    let bookmark = BookmarkName::new(bookmark_name).unwrap();
    let maybe_bcs_id = repo.get_bonsai_bookmark(ctx.clone(), &bookmark).await?;
    info!(
        ctx.logger(),
        "Current position of {:?} is {:?}", bookmark, maybe_bcs_id
    );
    match maybe_bcs_id {
        Some(bcs_id) => {
            let mut transaction = repo.update_bookmark_transaction(ctx);
            transaction.delete(&bookmark, bcs_id, BookmarkUpdateReason::ManualMove)?;
            transaction.commit().await?;
            Ok(())
        }
        None => Err(format_err!("Cannot delete missing bookmark")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_output_format() {
        let expected_answer = json!({
            "changeset_type": "hg",
            "changeset_id": "123"
        });
        assert_eq!(
            format_output(true, "123".to_string(), "hg"),
            to_string_pretty(&expected_answer).unwrap()
        );
    }

    #[test]
    fn plain_output_format() {
        assert_eq!(format_output(false, "123".to_string(), "hg"), "(HG) 123");
    }
}
