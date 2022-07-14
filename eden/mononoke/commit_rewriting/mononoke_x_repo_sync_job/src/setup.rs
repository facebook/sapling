/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use blobrepo::BlobRepo;
use clap_old::ArgMatches;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use context::CoreContext;
use mononoke_types::ChangesetId;
use std::time::Duration;

use scuba_ext::MononokeScubaSampleBuilder;

use crate::cli::ARG_COMMIT;
use crate::cli::ARG_LOG_TO_SCUBA;
use crate::cli::ARG_SLEEP_SECS;
use crate::reporting::SCUBA_TABLE;

const DEFAULT_SLEEP_SECS: u64 = 10;

pub async fn get_starting_commit<'a>(
    ctx: &CoreContext,
    matches: &ArgMatches<'a>,
    blobrepo: BlobRepo,
) -> Result<ChangesetId, Error> {
    let str_value = matches
        .value_of(ARG_COMMIT)
        .ok_or_else(|| format_err!("{} argument is required", ARG_COMMIT))
        .map(|s| s.to_owned())?;
    helpers::csid_resolve(ctx, &blobrepo, str_value).await
}

pub fn get_scuba_sample<'a>(
    ctx: CoreContext,
    matches: &MononokeMatches<'a>,
) -> MononokeScubaSampleBuilder {
    let log_to_scuba = matches.is_present(ARG_LOG_TO_SCUBA);
    let mut scuba_sample = if log_to_scuba {
        MononokeScubaSampleBuilder::new(ctx.fb, SCUBA_TABLE)
    } else {
        MononokeScubaSampleBuilder::with_discard()
    };
    scuba_sample.add_common_server_data();
    scuba_sample
}

pub fn get_sleep_duration<'a>(matches: &ArgMatches<'a>) -> Result<Duration, Error> {
    let secs = match matches.value_of(ARG_SLEEP_SECS) {
        Some(sleep_secs_str) => sleep_secs_str
            .parse::<u64>()
            .map_err(|_| format_err!("{} must be a valid u64", ARG_SLEEP_SECS)),
        None => Ok(DEFAULT_SLEEP_SECS),
    }?;
    Ok(Duration::from_secs(secs))
}
