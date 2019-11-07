/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]
#![feature(process_exitcode_placeholder)]

use clap::{App, Arg, SubCommand};
use fbinit::FacebookInit;
use futures::IntoFuture;
use futures_ext::FutureExt;
use std::process::ExitCode;
use tokio::runtime::Runtime;

use cmdlib::args;
use context::CoreContext;
use slog::error;

use crate::blobstore_fetch::subcommand_blobstore_fetch;
use crate::bonsai_fetch::subcommand_bonsai_fetch;
use crate::cmdargs::{
    ADD_PUBLIC_PHASES, BLOBSTORE_FETCH, BONSAI_FETCH, BOOKMARKS, CONTENT_FETCH, CROSSREPO,
    FETCH_PHASE, FILENODES, FILESTORE, HASH_CONVERT, HG_CHANGESET, HG_CHANGESET_DIFF,
    HG_CHANGESET_RANGE, HG_SYNC_BUNDLE, HG_SYNC_FETCH_BUNDLE, HG_SYNC_LAST_PROCESSED,
    HG_SYNC_REMAINS, HG_SYNC_SHOW, HG_SYNC_VERIFY, LIST_PUBLIC, PHASES, REDACTION, REDACTION_ADD,
    REDACTION_LIST, REDACTION_REMOVE, SKIPLIST, SKIPLIST_BUILD, SKIPLIST_READ, UNODES,
};
use crate::content_fetch::subcommand_content_fetch;
use crate::crossrepo::subcommand_crossrepo;
use crate::error::SubcommandError;
use crate::filenodes::subcommand_filenodes;
use crate::hash_convert::subcommand_hash_convert;
use crate::hg_changeset::subcommand_hg_changeset;
use crate::hg_sync::subcommand_process_hg_sync;
use crate::redaction::subcommand_redaction;
use crate::skiplist_subcommand::subcommand_skiplist;

mod blobstore_fetch;
mod bonsai_fetch;
mod bookmarks_manager;
mod cmdargs;
mod common;
mod content_fetch;
mod crossrepo;
mod error;
mod filenodes;
mod filestore;
mod hash_convert;
mod hg_changeset;
mod hg_sync;
mod phases;
mod redaction;
mod skiplist_subcommand;
mod subcommand_unodes;

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let blobstore_fetch = SubCommand::with_name(BLOBSTORE_FETCH)
        .about("fetches blobs from manifold")
        .args_from_usage("[KEY]    'key of the blob to be fetched'")
        .arg(
            Arg::with_name("decode-as")
                .long("decode-as")
                .short("d")
                .takes_value(true)
                .possible_values(&["auto", "changeset", "manifest", "file", "contents"])
                .required(false)
                .help("if provided decode the value"),
        )
        .arg(
            Arg::with_name("use-memcache")
                .long("use-memcache")
                .short("m")
                .takes_value(true)
                .possible_values(&["cache-only", "no-fill", "fill-mc"])
                .required(false)
                .help("Use memcache to cache access to the blob store"),
        )
        .arg(
            Arg::with_name("no-prefix")
                .long("no-prefix")
                .short("P")
                .takes_value(false)
                .required(false)
                .help("Don't prepend a prefix based on the repo id to the key"),
        )
        .arg(
            Arg::with_name("inner-blobstore-id")
                .long("inner-blobstore-id")
                .takes_value(true)
                .required(false)
                .help("If main blobstore in the storage config is a multiplexed one, use inner blobstore with this id")
        );

    let content_fetch = SubCommand::with_name(CONTENT_FETCH)
        .about("fetches content of the file or manifest from blobrepo")
        .args_from_usage(
            "<CHANGESET_ID>    'hg/bonsai id or bookmark to fetch file from'
             <PATH>            'path to fetch'",
        );

    let bonsai_fetch = SubCommand::with_name(BONSAI_FETCH)
        .about("fetches content of the file or manifest from blobrepo")
        .args_from_usage(
            r#"<CHANGESET_ID>    'hg/bonsai id or bookmark to fetch file from'
                          --json            'if provided json will be returned'"#,
        );

    let hg_changeset = SubCommand::with_name(HG_CHANGESET)
        .about("mercural changeset level queries")
        .subcommand(
            SubCommand::with_name(HG_CHANGESET_DIFF)
                .about("compare two changeset (used by pushrebase replayer)")
                .args_from_usage(
                    "<LEFT_CS>  'left changeset id'
                     <RIGHT_CS> 'right changeset id'",
                ),
        )
        .subcommand(
            SubCommand::with_name(HG_CHANGESET_RANGE)
                .about("returns `x::y` revset")
                .args_from_usage(
                    "<START_CS> 'start changeset id'
                     <STOP_CS>  'stop changeset id'",
                ),
        );

    let skiplist = SubCommand::with_name(SKIPLIST)
        .about("commands to build or read skiplist indexes")
        .subcommand(
            SubCommand::with_name(SKIPLIST_BUILD)
                .about("build skiplist index")
                .args_from_usage(
                    "<BLOBSTORE_KEY>  'Blobstore key where to store the built skiplist'",
                ),
        )
        .subcommand(
            SubCommand::with_name(SKIPLIST_READ)
                .about("read skiplist index")
                .args_from_usage(
                    "<BLOBSTORE_KEY>  'Blobstore key from where to read the skiplist'",
                ),
        );

    let convert = SubCommand::with_name(HASH_CONVERT)
        .about("convert between bonsai and hg changeset hashes")
        .arg(
            Arg::with_name("from")
                .long("from")
                .short("f")
                .required(true)
                .takes_value(true)
                .possible_values(&["hg", "bonsai"])
                .help("Source hash type"),
        )
        .arg(
            Arg::with_name("to")
                .long("to")
                .short("t")
                .required(true)
                .takes_value(true)
                .possible_values(&["hg", "bonsai"])
                .help("Target hash type"),
        )
        .args_from_usage("<HASH>  'source hash'");

    let hg_sync = SubCommand::with_name(HG_SYNC_BUNDLE)
        .about("things related to mononoke-hg-sync counters")
        .subcommand(
            SubCommand::with_name(HG_SYNC_LAST_PROCESSED)
                .about("inspect/change mononoke-hg sync last processed counter")
                .arg(
                    Arg::with_name("set")
                        .long("set")
                        .required(false)
                        .takes_value(true)
                        .help("set the value of the latest processed mononoke-hg-sync counter"),
                )
                .arg(
                    Arg::with_name("skip-blobimport")
                        .long("skip-blobimport")
                        .required(false)
                        .help("skip to the next non-blobimport entry in mononoke-hg-sync counter"),
                )
                .arg(
                    Arg::with_name("dry-run")
                        .long("dry-run")
                        .required(false)
                        .help("don't make changes, only show what would have been done (--skip-blobimport only)"),
                ),
        )
        .subcommand(
            SubCommand::with_name(HG_SYNC_REMAINS)
                .about("get the value of the last mononoke-hg-sync counter to be processed")
                .arg(
                    Arg::with_name("quiet")
                        .long("quiet")
                        .required(false)
                        .takes_value(false)
                        .help("only print the number if present"),
                )
                .arg(
                    Arg::with_name("without-blobimport")
                        .long("without-blobimport")
                        .required(false)
                        .takes_value(false)
                        .help("exclude blobimport entries from the count"),
                ),
        )
        .subcommand(
            SubCommand::with_name(HG_SYNC_SHOW).about("show hg hashes of yet to be replayed bundles")
                .arg(
                    Arg::with_name("limit")
                        .long("limit")
                        .required(false)
                        .takes_value(true)
                        .help("how many bundles to show"),
                )
        )
        .subcommand(
            SubCommand::with_name(HG_SYNC_FETCH_BUNDLE)
                .about("fetches a bundle by id")
                .arg(
                    Arg::with_name("id")
                        .long("id")
                        .required(true)
                        .takes_value(true)
                        .help("bookmark log id. If it has associated bundle it will be fetched."),
                )
                .arg(
                    Arg::with_name("output-file")
                        .long("output-file")
                        .required(true)
                        .takes_value(true)
                        .help("where a bundle will be saved"),
                ),
        )
        .subcommand(
            SubCommand::with_name(HG_SYNC_VERIFY)
                .about("verify the consistency of yet-to-be-processed bookmark log entries"),
        );

    let phases = SubCommand::with_name(PHASES)
        .about("commands to work with phases")
        .subcommand(
            SubCommand::with_name(ADD_PUBLIC_PHASES)
                .about("mark mercurial commits as public from provided new-line separated list")
                .arg(
                    Arg::with_name("input-file")
                        .help("new-line separated mercurial public commits")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::with_name("chunk-size")
                        .help("partition input file to chunks of specified size")
                        .long("chunk-size")
                        .takes_value(true),
                ),
        )
        .subcommand(
            SubCommand::with_name(FETCH_PHASE)
                .about("fetch phase of a commit")
                .arg(
                    Arg::with_name("changeset-type")
                        .long("changeset-type")
                        .short("c")
                        .takes_value(true)
                        .possible_values(&["bonsai", "hg"])
                        .required(false)
                        .help(
                            "What changeset type to return, either bonsai or hg. Defaults to hg.",
                        ),
                )
                .arg(
                    Arg::with_name("hash")
                        .help("changeset hash")
                        .takes_value(true),
                ),
        )
        .subcommand(
            SubCommand::with_name(LIST_PUBLIC)
                .arg(
                    Arg::with_name("changeset-type")
                        .long("changeset-type")
                        .short("c")
                        .takes_value(true)
                        .possible_values(&["bonsai", "hg"])
                        .required(false)
                        .help(
                            "What changeset type to return, either bonsai or hg. Defaults to hg.",
                        ),
                )
                .about("List all public commits"),
        );

    let redaction = SubCommand::with_name(REDACTION)
        .about("handle file redaction")
        .subcommand(
            SubCommand::with_name(REDACTION_ADD)
                .about("add a new redacted file at a given commit")
                .arg(
                    Arg::with_name("task")
                        .help("Task tracking the redaction request")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("hash")
                        .help("hg commit hash")
                        .takes_value(true)
                        .required(true),
                )
                .args_from_usage(
                    r#"
                        <FILES_LIST>...                             'list of files to be be redacted'
                        "#,
                )
        )
        .subcommand(
            SubCommand::with_name(REDACTION_REMOVE)
                .about("remove a file from the redaction")
                .arg(
                    Arg::with_name("hash")
                        .help("hg commit hash")
                        .takes_value(true)
                        .required(true),
                )
                .args_from_usage(
                    r#"
                        <FILES_LIST>...                             'list of files to be be unredacted'
                        "#,
                )
        )
        .subcommand(
            SubCommand::with_name(REDACTION_LIST)
                .about("list all redacted file for a given commit")
                .arg(
                    Arg::with_name("hash")
                        .help("hg commit hash or a bookmark")
                        .takes_value(true)
                        .required(true),
                )
        );

    args::MononokeApp::new("Mononoke admin command line tool")
        .with_advanced_args_hidden()
        .with_source_and_target_repos()
        .build()
        .version("0.0.0")
        .about("Poke at mononoke internals for debugging and investigating data structures.")
        .subcommand(blobstore_fetch)
        .subcommand(bonsai_fetch)
        .subcommand(content_fetch)
        .subcommand(bookmarks_manager::prepare_command(SubCommand::with_name(
            BOOKMARKS,
        )))
        .subcommand(hg_changeset)
        .subcommand(skiplist)
        .subcommand(convert)
        .subcommand(hg_sync)
        .subcommand(redaction)
        .subcommand(filenodes::build_subcommand(FILENODES))
        .subcommand(phases)
        .subcommand(filestore::build_subcommand(FILESTORE))
        .subcommand(subcommand_unodes::subcommand_unodes_build(UNODES))
        .subcommand(crossrepo::build_subcommand(CROSSREPO))
}

#[fbinit::main]
fn main(fb: FacebookInit) -> ExitCode {
    let matches = setup_app().get_matches();

    let logger = args::init_logging(fb, &matches);
    let error_logger = logger.clone();

    let future = match matches.subcommand() {
        (BLOBSTORE_FETCH, Some(sub_m)) => subcommand_blobstore_fetch(fb, logger, &matches, sub_m),
        (BONSAI_FETCH, Some(sub_m)) => subcommand_bonsai_fetch(fb, logger, &matches, sub_m),
        (CONTENT_FETCH, Some(sub_m)) => subcommand_content_fetch(fb, logger, &matches, sub_m),
        (BOOKMARKS, Some(sub_m)) => {
            args::init_cachelib(fb, &matches);
            let ctx = CoreContext::new_with_logger(fb, logger.clone());
            let repo_fut = args::open_repo(fb, &logger, &matches).boxify();
            bookmarks_manager::handle_command(ctx, repo_fut, sub_m, logger)
        }
        (HG_CHANGESET, Some(sub_m)) => subcommand_hg_changeset(fb, logger, &matches, sub_m),
        (HG_SYNC_BUNDLE, Some(sub_m)) => {
            subcommand_process_hg_sync(fb, sub_m, &matches, logger.clone())
        }
        (SKIPLIST, Some(sub_m)) => subcommand_skiplist(fb, logger, &matches, sub_m),
        (HASH_CONVERT, Some(sub_m)) => subcommand_hash_convert(fb, logger, &matches, sub_m),
        (REDACTION, Some(sub_m)) => subcommand_redaction(fb, logger, &matches, sub_m),
        (FILENODES, Some(sub_m)) => subcommand_filenodes(fb, logger, &matches, sub_m),
        (FILESTORE, Some(sub_m)) => filestore::execute_command(fb, logger, &matches, sub_m),
        (PHASES, Some(sub_m)) => phases::subcommand_phases(fb, logger, &matches, sub_m),
        (UNODES, Some(sub_m)) => subcommand_unodes::subcommand_unodes(fb, logger, &matches, sub_m),
        (CROSSREPO, Some(sub_m)) => subcommand_crossrepo(fb, logger, &matches, sub_m),
        _ => Err(SubcommandError::InvalidArgs).into_future().boxify(),
    };

    let debug = matches.is_present("debug");

    let mut runtime = Runtime::new().expect("failed to initialize Tokio runtime");
    let res = runtime.block_on(future);

    match res {
        Ok(_) => ExitCode::SUCCESS,
        Err(SubcommandError::Error(err)) => {
            error!(error_logger, "{:?}", err);
            if debug {
                error!(error_logger, "\n============ DEBUG ERROR ============");
                error!(error_logger, "{:#?}", err);
            }
            ExitCode::FAILURE
        }
        Err(SubcommandError::InvalidArgs) => {
            eprintln!("{}", matches.usage());
            ExitCode::FAILURE
        }
    }
}
