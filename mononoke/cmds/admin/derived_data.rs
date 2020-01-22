/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::pin::Pin;

use blobrepo::BlobRepo;
use clap::{App, Arg, ArgMatches, SubCommand};
use cmdlib::{args, helpers::csid_resolve};
use context::CoreContext;
use derived_data_utils::{derived_data_utils, POSSIBLE_DERIVED_TYPES};
use fbinit::FacebookInit;
use futures_preview::{compat::Future01CompatExt, future::FutureExt as PreviewFutureExt, Future};
use futures_util::future::try_join_all;
use slog::Logger;

use crate::error::SubcommandError;

const SUBCOMMAND_EXISTS: &'static str = "exists";

const ARG_HASH_OR_BOOKMARK: &'static str = "hash-or-bookmark";
const ARG_TYPE: &'static str = "type";

pub fn build_subcommand(name: &str) -> App {
    SubCommand::with_name(name)
        .about("request information about derived data")
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_EXISTS)
                .about("check if derived data has been generated")
                .arg(
                    Arg::with_name(ARG_TYPE)
                        .help("type of derived data")
                        .takes_value(true)
                        .possible_values(POSSIBLE_DERIVED_TYPES)
                        .required(true),
                )
                .arg(
                    Arg::with_name(ARG_HASH_OR_BOOKMARK)
                        .help("(hg|bonsai) commit hash or bookmark")
                        .takes_value(true)
                        .multiple(true)
                        .required(true),
                ),
        )
}

pub fn subcommand_derived_data(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> Pin<Box<dyn Future<Output = Result<(), SubcommandError>> + Send>> {
    args::init_cachelib(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo = args::open_repo(fb, &logger, &matches);

    match sub_m.subcommand() {
        (SUBCOMMAND_EXISTS, Some(arg_matches)) => {
            let hashes_or_bookmarks: Vec<_> = arg_matches
                .values_of(ARG_HASH_OR_BOOKMARK)
                .map(|matches| matches.map(|cs| cs.to_string()).collect())
                .unwrap();

            let derived_data_type = arg_matches
                .value_of(ARG_TYPE)
                .map(|m| m.to_string())
                .unwrap();

            async move {
                let repo = repo.compat().await?;
                check_derived_data_exists(ctx, repo, derived_data_type, hashes_or_bookmarks).await
            }
                .boxed()
        }
        _ => async move { Err(SubcommandError::InvalidArgs) }.boxed(),
    }
}

async fn check_derived_data_exists(
    ctx: CoreContext,
    repo: BlobRepo,
    derived_data_type: String,
    hashes_or_bookmarks: Vec<String>,
) -> Result<(), SubcommandError> {
    let derived_utils = derived_data_utils(ctx.clone(), repo.clone(), derived_data_type)?;

    let cs_id_futs: Vec<_> = hashes_or_bookmarks
        .into_iter()
        .map(|hash_or_bm| csid_resolve(ctx.clone(), repo.clone(), hash_or_bm).compat())
        .collect();

    let cs_ids = try_join_all(cs_id_futs).await?;

    let pending = derived_utils
        .pending(ctx.clone(), repo.clone(), cs_ids.clone())
        .compat()
        .await?;

    for cs_id in cs_ids {
        if pending.contains(&cs_id) {
            println!("Not Derived: {}", cs_id);
        } else {
            println!("Derived: {}", cs_id);
        }
    }

    Ok(())
}
