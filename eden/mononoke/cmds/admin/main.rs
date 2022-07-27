/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "8000000"]
#![feature(btree_drain_filter)]

use blobstore::PutBehaviour;
use fbinit::FacebookInit;
use std::process::ExitCode;

use cmdlib::args;
use cmdlib::args::ArgType;
use cmdlib::args::MononokeClapApp;
use context::CoreContext;
use slog::error;

use crate::blobstore_fetch::subcommand_blobstore_fetch;
use crate::blobstore_unlink::subcommand_blobstore_unlink;
use crate::blobstore_upload::subcommand_blobstore_upload;
use crate::bonsai_fetch::subcommand_bonsai_fetch;
use crate::content_fetch::subcommand_content_fetch;
use crate::crossrepo::subcommand_crossrepo;
use crate::error::SubcommandError;
use crate::filenodes::subcommand_filenodes;
use crate::hash_convert::subcommand_hash_convert;
use crate::hg_changeset::subcommand_hg_changeset;
use crate::mutable_counters::subcommand_mutable_counters;
use crate::redaction::subcommand_redaction;
use crate::skiplist_subcommand::subcommand_skiplist;

mod blobstore_fetch;
mod blobstore_unlink;
mod blobstore_upload;
mod bonsai_fetch;
mod bookmarks_manager;
mod common;
mod content_fetch;
mod crossrepo;
mod derived_data;
mod error;
mod filenodes;
mod hash_convert;
mod hg_changeset;
mod mutable_counters;
mod redaction;
mod rsync;
mod skiplist_subcommand;
mod subcommand_blame;
mod subcommand_deleted_manifest;
mod subcommand_fsnodes;
mod subcommand_phases;
mod subcommand_skeleton_manifests;
mod subcommand_unodes;
mod truncate_segmented_changelog;

fn setup_app<'a, 'b>() -> MononokeClapApp<'a, 'b> {
    args::MononokeAppBuilder::new("Mononoke admin command line tool")
        .with_arg_types(vec![ArgType::Scrub])
        .with_advanced_args_hidden()
        .with_source_and_target_repos()
        .with_special_put_behaviour(PutBehaviour::Overwrite)
        .build()
        .about("Poke at mononoke internals for debugging and investigating data structures.")
        .subcommand(blobstore_fetch::build_subcommand())
        .subcommand(blobstore_unlink::build_subcommand())
        .subcommand(blobstore_upload::build_subcommand())
        .subcommand(bonsai_fetch::build_subcommand())
        .subcommand(content_fetch::build_subcommand())
        .subcommand(bookmarks_manager::build_subcommand())
        .subcommand(hg_changeset::build_subcommand())
        .subcommand(skiplist_subcommand::build_subcommand())
        .subcommand(hash_convert::build_subcommand())
        .subcommand(mutable_counters::build_subcommand())
        .subcommand(redaction::build_subcommand())
        .subcommand(filenodes::build_subcommand())
        .subcommand(subcommand_phases::build_subcommand())
        .subcommand(subcommand_unodes::build_subcommand())
        .subcommand(subcommand_fsnodes::build_subcommand())
        .subcommand(crossrepo::build_subcommand())
        .subcommand(subcommand_blame::build_subcommand())
        .subcommand(subcommand_deleted_manifest::build_subcommand())
        .subcommand(derived_data::build_subcommand())
        .subcommand(rsync::build_subcommand())
        .subcommand(subcommand_skeleton_manifests::build_subcommand())
        .subcommand(truncate_segmented_changelog::build_subcommand())
}

#[fbinit::main]
fn main(fb: FacebookInit) -> ExitCode {
    let matches = setup_app()
        .get_matches(fb)
        .expect("Failed to start Mononoke");

    let logger = matches.logger().clone();
    let error_logger = logger.clone();

    let debug = matches.is_present("debug");

    let runtime = matches.runtime();
    let res = runtime.block_on(async {
        match matches.subcommand() {
            (blobstore_fetch::BLOBSTORE_FETCH, Some(sub_m)) => {
                subcommand_blobstore_fetch(fb, logger, &matches, sub_m).await
            }
            (blobstore_unlink::BLOBSTORE_UNLINK, Some(sub_m)) => {
                subcommand_blobstore_unlink(fb, logger, &matches, sub_m).await
            }
            (blobstore_upload::BLOBSTORE_UPLOAD, Some(sub_m)) => {
                subcommand_blobstore_upload(fb, logger, &matches, sub_m).await
            }
            (bonsai_fetch::BONSAI_FETCH, Some(sub_m)) => {
                subcommand_bonsai_fetch(fb, logger, &matches, sub_m).await
            }
            (content_fetch::CONTENT_FETCH, Some(sub_m)) => {
                subcommand_content_fetch(fb, logger, &matches, sub_m).await
            }
            (bookmarks_manager::BOOKMARKS, Some(sub_m)) => {
                let ctx = CoreContext::new_with_logger(fb, logger.clone());
                let repo = args::open_repo(fb, &logger, &matches).await?;
                bookmarks_manager::handle_command(ctx, repo, sub_m, logger.clone()).await
            }
            (hg_changeset::HG_CHANGESET, Some(sub_m)) => {
                subcommand_hg_changeset(fb, logger, &matches, sub_m).await
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
            (subcommand_phases::PHASES, Some(sub_m)) => {
                subcommand_phases::subcommand_phases(fb, logger, &matches, sub_m).await
            }
            (subcommand_unodes::UNODES, Some(sub_m)) => {
                subcommand_unodes::subcommand_unodes(fb, logger, &matches, sub_m).await
            }
            (subcommand_fsnodes::FSNODES, Some(sub_m)) => {
                subcommand_fsnodes::subcommand_fsnodes(fb, logger, &matches, sub_m).await
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
            (rsync::RSYNC, Some(sub_m)) => {
                rsync::subcommand_rsync(fb, logger, &matches, sub_m).await
            }
            (subcommand_skeleton_manifests::SKELETON_MANIFESTS, Some(sub_m)) => {
                subcommand_skeleton_manifests::subcommand_skeleton_manifests(
                    fb, logger, &matches, sub_m,
                )
                .await
            }
            (truncate_segmented_changelog::TRUNCATE_SEGMENTED_CHANGELOG, Some(sub_m)) => {
                truncate_segmented_changelog::subcommand_truncate_segmented_changelog(
                    fb, logger, &matches, sub_m,
                )
                .await
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
