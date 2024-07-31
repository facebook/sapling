/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "8000000"]

use std::process::ExitCode;

use blobstore::PutBehaviour;
use cmdlib::args;
use cmdlib::args::ArgType;
use cmdlib::args::MononokeClapApp;
use fbinit::FacebookInit;
use slog::error;

use crate::crossrepo::subcommand_crossrepo;
use crate::error::SubcommandError;
use crate::filenodes::subcommand_filenodes;
use crate::hg_changeset::subcommand_hg_changeset;

mod common;
mod crossrepo;
mod error;
mod filenodes;
mod hg_changeset;
mod rsync;
mod subcommand_blame;
mod subcommand_deleted_manifest;
mod subcommand_phases;

fn setup_app<'a, 'b>() -> MononokeClapApp<'a, 'b> {
    args::MononokeAppBuilder::new("Mononoke admin command line tool")
        .with_arg_types(vec![ArgType::Scrub])
        .with_advanced_args_hidden()
        .with_source_and_target_repos()
        .with_special_put_behaviour(PutBehaviour::Overwrite)
        .build()
        .about("Poke at mononoke internals for debugging and investigating data structures.")
        .subcommand(hg_changeset::build_subcommand())
        .subcommand(filenodes::build_subcommand())
        .subcommand(subcommand_phases::build_subcommand())
        .subcommand(crossrepo::build_subcommand())
        .subcommand(subcommand_blame::build_subcommand())
        .subcommand(subcommand_deleted_manifest::build_subcommand())
        .subcommand(rsync::build_subcommand())
}

#[fbinit::main]
fn main(fb: FacebookInit) -> ExitCode {
    let (matches, runtime) = setup_app()
        .get_matches(fb)
        .expect("Failed to start Mononoke");

    let logger = matches.logger().clone();
    let error_logger = logger.clone();

    let debug = matches.is_present("debug");

    let res = runtime.block_on(async {
        match matches.subcommand() {
            (hg_changeset::HG_CHANGESET, Some(sub_m)) => {
                subcommand_hg_changeset(fb, logger, &matches, sub_m).await
            }
            (filenodes::FILENODES, Some(sub_m)) => {
                subcommand_filenodes(fb, logger, &matches, sub_m).await
            }
            (subcommand_phases::PHASES, Some(sub_m)) => {
                subcommand_phases::subcommand_phases(fb, logger, &matches, sub_m).await
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
            (rsync::RSYNC, Some(sub_m)) => {
                rsync::subcommand_rsync(fb, logger, &matches, sub_m).await
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
