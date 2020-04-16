/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::cmdargs::{
    MUTABLE_COUNTERS_GET, MUTABLE_COUNTERS_LIST, MUTABLE_COUNTERS_NAME, MUTABLE_COUNTERS_SET,
    MUTABLE_COUNTERS_VALUE,
};
use crate::error::SubcommandError;
use anyhow::{format_err, Error};

use clap::ArgMatches;
use cmdlib::args;
use context::CoreContext;
use failure_ext::FutureFailureErrorExt;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use mononoke_types::RepositoryId;
use mutable_counters::{MutableCounters, SqlMutableCounters};
use slog::{info, Logger};

pub async fn subcommand_mutable_counters<'a>(
    fb: FacebookInit,
    sub_m: &'a ArgMatches<'_>,
    matches: &'a ArgMatches<'_>,
    logger: Logger,
) -> Result<(), SubcommandError> {
    let repo_id = args::get_repo_id(fb, &matches)?;

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let mutable_counters = args::open_sql::<SqlMutableCounters>(fb, &matches)
        .context("While opening SqlMutableCounters")
        .compat()
        .await?;

    match sub_m.subcommand() {
        (MUTABLE_COUNTERS_LIST, Some(_)) => {
            mutable_counters_list(ctx, repo_id, mutable_counters).await
        }
        (MUTABLE_COUNTERS_GET, Some(sub_m)) => {
            let name = sub_m
                .value_of(MUTABLE_COUNTERS_NAME)
                .ok_or_else(|| format_err!("counter name is required"))?;

            mutable_counters_get(ctx, repo_id, name, mutable_counters).await
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
    repo_id: RepositoryId,
    mutable_counters: SqlMutableCounters,
) -> Result<(), Error> {
    let counters = mutable_counters
        .get_all_counters(ctx.clone(), repo_id)
        .compat()
        .await?;

    for (name, value) in counters {
        println!("{:<30}={}", name, value);
    }

    Ok(())
}

async fn mutable_counters_get(
    ctx: CoreContext,
    repo_id: RepositoryId,
    name: &str,
    mutable_counters: SqlMutableCounters,
) -> Result<(), Error> {
    let maybe_value = mutable_counters
        .get_counter(ctx.clone(), repo_id, name)
        .compat()
        .await?;

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
        .set_counter(ctx.clone(), repo_id, name, value, None)
        .compat()
        .await?;

    info!(
        ctx.logger(),
        "Value of {} in {} set to {}", name, repo_id, value
    );
    Ok(())
}
