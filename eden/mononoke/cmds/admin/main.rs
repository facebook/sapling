/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "8000000"]
#![deny(warnings)]
#![feature(process_exitcode_placeholder)]

use clap::App;
use fbinit::FacebookInit;
use futures_ext::FutureExt as Future01Ext;
use std::process::ExitCode;

use cmdlib::args;
use context::CoreContext;
use slog::error;

use crate::blobstore_fetch::subcommand_blobstore_fetch;
use crate::bonsai_fetch::subcommand_bonsai_fetch;
use crate::content_fetch::subcommand_content_fetch;
use crate::crossrepo::subcommand_crossrepo;
use crate::error::SubcommandError;
use crate::filenodes::subcommand_filenodes;
use crate::hash_convert::subcommand_hash_convert;
use crate::hg_changeset::subcommand_hg_changeset;
use crate::hg_sync::subcommand_process_hg_sync;
use crate::mutable_counters::subcommand_mutable_counters;
use crate::redaction::subcommand_redaction;
use crate::skiplist_subcommand::subcommand_skiplist;

mod blobstore_fetch;
mod bonsai_fetch;
mod bookmarks_manager;
mod common;
mod content_fetch;
mod crossrepo;
mod derived_data;
mod error;
mod filenodes;
mod filestore;
mod hash_convert;
mod hg_changeset;
mod hg_sync;
mod mutable_counters;
mod phases;
mod redaction;
mod skiplist_subcommand;
mod subcommand_blame;
mod subcommand_deleted_manifest;
mod subcommand_unodes;

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    args::MononokeApp::new("Mononoke admin command line tool")
        .with_advanced_args_hidden()
        .with_test_args()
        .with_source_and_target_repos()
        .build()
        .version("0.0.0")
        .about("Poke at mononoke internals for debugging and investigating data structures.")
        .subcommand(blobstore_fetch::build_subcommand())
        .subcommand(bonsai_fetch::build_subcommand())
        .subcommand(content_fetch::build_subcommand())
        .subcommand(bookmarks_manager::build_subcommand())
        .subcommand(hg_changeset::build_subcommand())
        .subcommand(skiplist_subcommand::build_subcommand())
        .subcommand(hash_convert::build_subcommand())
        .subcommand(hg_sync::build_subcommand())
        .subcommand(mutable_counters::build_subcommand())
        .subcommand(redaction::build_subcommand())
        .subcommand(filenodes::build_subcommand())
        .subcommand(phases::build_subcommand())
        .subcommand(filestore::build_subcommand())
        .subcommand(subcommand_unodes::build_subcommand())
        .subcommand(crossrepo::build_subcommand())
        .subcommand(subcommand_blame::build_subcommand())
        .subcommand(subcommand_deleted_manifest::build_subcommand())
        .subcommand(derived_data::build_subcommand())
}

#[fbinit::main]
fn main(fb: FacebookInit) -> ExitCode {
    let matches = setup_app().get_matches();

    let logger = args::init_logging(fb, &matches);
    let error_logger = logger.clone();

    args::init_tunables(fb, &matches, logger.clone()).expect("failed to initialise tunables");

    let debug = matches.is_present("debug");

    let mut runtime = args::init_runtime(&matches).expect("failed to initialize Tokio runtime");
    let res = runtime.block_on_std(async {
        match matches.subcommand() {
            (blobstore_fetch::BLOBSTORE_FETCH, Some(sub_m)) => {
                subcommand_blobstore_fetch(fb, logger, &matches, sub_m).await
            }
            (bonsai_fetch::BONSAI_FETCH, Some(sub_m)) => {
                subcommand_bonsai_fetch(fb, logger, &matches, sub_m).await
            }
            (content_fetch::CONTENT_FETCH, Some(sub_m)) => {
                subcommand_content_fetch(fb, logger, &matches, sub_m).await
            }
            (bookmarks_manager::BOOKMARKS, Some(sub_m)) => {
                args::init_cachelib(fb, &matches, None);
                let ctx = CoreContext::new_with_logger(fb, logger.clone());
                let repo_fut = args::open_repo(fb, &logger, &matches).boxify();
                bookmarks_manager::handle_command(ctx, repo_fut, sub_m, logger).await
            }
            (hg_changeset::HG_CHANGESET, Some(sub_m)) => {
                subcommand_hg_changeset(fb, logger, &matches, sub_m).await
            }
            (hg_sync::HG_SYNC_BUNDLE, Some(sub_m)) => {
                subcommand_process_hg_sync(fb, sub_m, &matches, logger.clone()).await
            }
            (skiplist_subcommand::SKIPLIST, Some(sub_m)) => {
                subcommand_skiplist(fb, logger, &matches, sub_m).await
            }
            (hash_convert::HASH_CONVERT, Some(sub_m)) => {
                subcommand_hash_convert(fb, logger, &matches, sub_m).await
            }
            (mutable_counters::MUTABLE_COUNTERS, Some(sub_m)) => {
                subcommand_mutable_counters(fb, sub_m, &matches, logger.clone()).await
            }
            (redaction::REDACTION, Some(sub_m)) => {
                subcommand_redaction(fb, logger, &matches, sub_m).await
            }
            (filenodes::FILENODES, Some(sub_m)) => {
                subcommand_filenodes(fb, logger, &matches, sub_m).await
            }
            (filestore::FILESTORE, Some(sub_m)) => {
                filestore::execute_command(fb, logger, &matches, sub_m).await
            }
            (phases::PHASES, Some(sub_m)) => {
                phases::subcommand_phases(fb, logger, &matches, sub_m).await
            }
            (subcommand_unodes::UNODES, Some(sub_m)) => {
                subcommand_unodes::subcommand_unodes(fb, logger, &matches, sub_m).await
            }
            (crossrepo::CROSSREPO, Some(sub_m)) => {
                subcommand_crossrepo(fb, logger, &matches, sub_m).await
            }
            (subcommand_blame::BLAME, Some(sub_m)) => {
                subcommand_blame::subcommand_blame(fb, logger, &matches, sub_m).await
            }
            (subcommand_deleted_manifest::DELETED_MANIFEST, Some(sub_m)) => {
                subcommand_deleted_manifest::subcommand_deleted_manifest(
                    fb, logger, &matches, sub_m,
                )
                .await
            }
            (derived_data::DERIVED_DATA, Some(sub_m)) => {
                derived_data::subcommand_derived_data(fb, logger, &matches, sub_m).await
            }
            _ => Err(SubcommandError::InvalidArgs),
        }
    });

    match res {
        Ok(_) => ExitCode::SUCCESS,
        Err(SubcommandError::Error(err)) => {
            if debug {
                error!(error_logger, "{:?}", err);
                error!(error_logger, "\n============ DEBUG ERROR ============");
                error!(error_logger, "{:#?}", err);
            } else {
                let mut err_string = format!("{:?}", err);
                if let Some(pos) = err_string.find("\n\nStack backtrace:") {
                    err_string.truncate(pos);
                }
                error!(error_logger, "{}", err_string);
            }
            ExitCode::FAILURE
        }
        Err(SubcommandError::InvalidArgs) => {
            eprintln!("{}", matches.usage());
            ExitCode::FAILURE
        }
    }
}
