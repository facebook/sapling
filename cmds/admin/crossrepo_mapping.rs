// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::{App, Arg, ArgMatches, SubCommand};
use fbinit::FacebookInit;
use futures::Future;
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};

use cmdlib::{args, helpers};
use context::CoreContext;
use slog::Logger;
use synced_commit_mapping::{SqlSyncedCommitMapping, SyncedCommitMapping};

use crate::error::SubcommandError;

const TARGET_REPO_NAME: &str = "target-repo-name";
const TARGET_REPO_ID: &str = "target-repo-id";

pub fn subcommand_crossrepo_map(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    let configs = try_boxfuture!(args::read_configs(matches));
    let source_repo = try_boxfuture!(args::get_repo_id(matches));
    let target_repo = try_boxfuture!(args::get_repo_id_and_name_from_values(
        sub_m.value_of(TARGET_REPO_NAME),
        sub_m.value_of(TARGET_REPO_ID),
        configs,
    ))
    .0;
    args::init_cachelib(fb, &matches);
    let repo = args::open_repo(fb, &logger, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let mapping = args::open_sql::<SqlSyncedCommitMapping>(&matches);
    let source_hash = repo.and_then({
        let ctx = ctx.clone();
        let hash = sub_m.value_of("HASH").unwrap().to_owned();
        move |repo| helpers::csid_resolve(ctx.clone(), repo, hash)
    });

    source_hash
        .join(mapping)
        .and_then(move |(source_hash, mapping)| {
            mapping
                .get(ctx, source_repo, source_hash, target_repo)
                .and_then(move |mapped| {
                    match mapped {
                        None => println!(
                            "Hash {} not currently remapped (could be present in target as-is)",
                            source_hash
                        ),
                        Some(target_hash) => {
                            println!("Hash {} maps to {}", source_hash, target_hash)
                        }
                    };
                    Ok(())
                })
        })
        .from_err()
        .boxify()
}

pub fn build_subcommand(name: &str) -> App {
    SubCommand::with_name(name)
        .about("Check cross-repo commit mapping")
        .arg(Arg::from_usage("<HASH>  'bonsai changeset hash to check'"))
        .arg(
            Arg::with_name(TARGET_REPO_ID)
                .long(TARGET_REPO_ID)
                .value_name("ID")
                .help("numeric ID of target repository"),
        )
        .arg(
            Arg::with_name(TARGET_REPO_NAME)
                .long(TARGET_REPO_NAME)
                .value_name("NAME")
                .help("Name of target repository"),
        )
}
