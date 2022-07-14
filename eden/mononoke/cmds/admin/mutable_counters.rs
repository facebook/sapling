/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::error::SubcommandError;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;

use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_types::RepositoryId;
use mutable_counters::MutableCounters;
use mutable_counters::SqlMutableCounters;
use mutable_counters::SqlMutableCountersBuilder;
use slog::info;
use slog::Logger;

pub const MUTABLE_COUNTERS: &str = "mutable-counters";
const MUTABLE_COUNTERS_NAME: &str = "name";
const MUTABLE_COUNTERS_VALUE: &str = "value";
const MUTABLE_COUNTERS_LIST: &str = "list";
const MUTABLE_COUNTERS_GET: &str = "get";
const MUTABLE_COUNTERS_SET: &str = "set";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(MUTABLE_COUNTERS)
        .about("handle mutable counters")
        .subcommand(
            SubCommand::with_name(MUTABLE_COUNTERS_LIST)
                .about("get all the mutable counters for a repo"),
        )
        .subcommand(
            SubCommand::with_name(MUTABLE_COUNTERS_GET)
                .about("get the value of the mutable counter")
                .arg(
                    Arg::with_name(MUTABLE_COUNTERS_NAME)
                        .help("name of the mutable counter to get")
                        .takes_value(true)
                        .required(true)
                        .index(1),
                ),
        )
        .subcommand(
            SubCommand::with_name(MUTABLE_COUNTERS_SET)
                .about("set the value of the mutable counter")
                .arg(
                    Arg::with_name(MUTABLE_COUNTERS_NAME)
                        .help("name of the mutable counter to set")
                        .takes_value(true)
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::with_name(MUTABLE_COUNTERS_VALUE)
                        .help("value of the mutable counter to set")
                        .takes_value(true)
                        .required(true)
                        .index(2),
                ),
        )
}

pub async fn subcommand_mutable_counters<'a>(
    fb: FacebookInit,
    sub_m: &'a ArgMatches<'_>,
    matches: &'a MononokeMatches<'_>,
    logger: Logger,
) -> Result<(), SubcommandError> {
    let config_store = matches.config_store();

    let repo_id = args::get_repo_id(config_store, matches)?;

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let mutable_counters = args::open_sql::<SqlMutableCountersBuilder>(fb, config_store, matches)
        .context("While opening SqlMutableCounters")?
        .build(repo_id);

    match sub_m.subcommand() {
        (MUTABLE_COUNTERS_LIST, Some(_)) => mutable_counters_list(ctx, mutable_counters).await,
        (MUTABLE_COUNTERS_GET, Some(sub_m)) => {
            let name = sub_m
                .value_of(MUTABLE_COUNTERS_NAME)
                .ok_or_else(|| format_err!("counter name is required"))?;

            mutable_counters_get(ctx, name, mutable_counters).await
        }
        (MUTABLE_COUNTERS_SET, Some(sub_m)) => {
            let name = sub_m
                .value_of(MUTABLE_COUNTERS_NAME)
                .ok_or_else(|| format_err!("{} is required", MUTABLE_COUNTERS_NAME))?;

            let value = args::get_i64_opt(sub_m, MUTABLE_COUNTERS_VALUE)
                .ok_or_else(|| format_err!("{} is required", MUTABLE_COUNTERS_VALUE))?;

            mutable_counters_set(ctx, repo_id, name, value, mutable_counters).await
        }
        (_, _) => Err(format_err!("unknown mutable_counters subcommand")),
    }
    .map_err(SubcommandError::from)
}

async fn mutable_counters_list(
    ctx: CoreContext,
    mutable_counters: SqlMutableCounters,
) -> Result<(), Error> {
    let counters = mutable_counters.get_all_counters(&ctx).await?;

    for (name, value) in counters {
        println!("{:<30}={}", name, value);
    }

    Ok(())
}

async fn mutable_counters_get(
    ctx: CoreContext,
    name: &str,
    mutable_counters: SqlMutableCounters,
) -> Result<(), Error> {
    let maybe_value = mutable_counters.get_counter(&ctx, name).await?;

    println!("{:?}", maybe_value);
    Ok(())
}

async fn mutable_counters_set(
    ctx: CoreContext,
    repo_id: RepositoryId,
    name: &str,
    value: i64,
    mutable_counters: SqlMutableCounters,
) -> Result<(), Error> {
    mutable_counters
        .set_counter(&ctx, name, value, None)
        .await?;

    info!(
        ctx.logger(),
        "Value of {} in {} set to {}", name, repo_id, value
    );
    Ok(())
}
