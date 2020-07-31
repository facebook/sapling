/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use clap::ArgMatches;
use cmdlib::helpers;
use context::CoreContext;
use futures_old::{Future, IntoFuture};
use mononoke_types::ChangesetId;
use std::sync::Arc;

use metaconfig_types::RepoConfig;
use scuba_ext::ScubaSampleBuilder;
use skiplist::{fetch_skiplist_index, SkiplistIndex};

use crate::cli::{ARG_COMMIT, ARG_LOG_TO_SCUBA, ARG_SLEEP_SECS, ARG_TARGET_BOOKMARK};
use crate::reporting::SCUBA_TABLE;

const DEFAULT_SLEEP_SECS: u64 = 10;

pub async fn get_skiplist_index(
    ctx: &CoreContext,
    config: &RepoConfig,
    repo: &BlobRepo,
) -> Result<Arc<SkiplistIndex>, Error> {
    fetch_skiplist_index(
        &ctx,
        &config.skiplist_index_blobstore_key,
        &Arc::new(repo.get_blobstore().boxed()),
    )
    .await
}

pub fn get_starting_commit<'a>(
    ctx: CoreContext,
    matches: &ArgMatches<'a>,
    blobrepo: BlobRepo,
) -> impl Future<Item = ChangesetId, Error = Error> {
    matches
        .value_of(ARG_COMMIT)
        .ok_or_else(|| format_err!("{} argument is required", ARG_COMMIT))
        .map(|s| s.to_owned())
        .into_future()
        .and_then(move |str_value| helpers::csid_resolve(ctx, blobrepo, str_value))
}

pub fn get_target_bookmark<'a>(matches: &ArgMatches<'a>) -> Result<BookmarkName, Error> {
    let name = matches
        .value_of(ARG_TARGET_BOOKMARK)
        .ok_or_else(|| format_err!("{} argument is required", ARG_TARGET_BOOKMARK))
        .map(|s| s.to_owned())?;

    BookmarkName::new(name)
}

pub fn get_scuba_sample<'a>(ctx: CoreContext, matches: &ArgMatches<'a>) -> ScubaSampleBuilder {
    let log_to_scuba = matches.is_present(ARG_LOG_TO_SCUBA);
    let mut scuba_sample = if log_to_scuba {
        ScubaSampleBuilder::new(ctx.fb, SCUBA_TABLE)
    } else {
        ScubaSampleBuilder::with_discard()
    };
    scuba_sample.add_common_server_data();
    scuba_sample
}

pub fn get_sleep_secs<'a>(matches: &ArgMatches<'a>) -> Result<u64, Error> {
    match matches.value_of(ARG_SLEEP_SECS) {
        Some(sleep_secs_str) => sleep_secs_str
            .parse::<u64>()
            .map_err(|_| format_err!("{} must be a valid u64", ARG_SLEEP_SECS)),
        None => Ok(DEFAULT_SLEEP_SECS),
    }
}
