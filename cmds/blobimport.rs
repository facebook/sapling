/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use ascii::AsciiString;
use blobimport_lib;
use bonsai_globalrev_mapping::SqlBonsaiGlobalrevMapping;
use clap::{App, Arg};
use cloned::cloned;
use cmdlib::{args, helpers};
use context::CoreContext;
use failure_ext::{err_msg, format_err, Error, Result, ResultExt, SlogKVError};
use fbinit::FacebookInit;
use futures::Future;
use futures_ext::FutureExt;
use mercurial_types::{HgChangesetId, HgNodeHash};
use phases::SqlPhases;
use slog::{error, warn, Logger};
use std::collections::HashMap;
use std::fs::read;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{trace_args, Traced};

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    args::MononokeApp::new("revlog to blob importer")
        .build()
        .version("0.0.0")
        .about("Import a revlog-backed Mercurial repo into Mononoke blobstore.")
        .args_from_usage(
            r#"
            <INPUT>                          'input revlog repo'
            --changeset [HASH]               'if provided, the only changeset to be imported'
            --no-bookmark                    'if provided won't update bookmarks'
            --prefix-bookmark [PREFIX]       'if provided will update bookmarks, but prefix them with PREFIX'
            --no-create                      'if provided won't create a new repo (only meaningful for local)'
            --lfs-helper [LFS_HELPER]        'if provided, path to an executable that accepts OID SIZE and returns a LFS blob to stdout'
            --concurrent-changesets [LIMIT]  'if provided, max number of changesets to upload concurrently'
            --concurrent-blobs [LIMIT]       'if provided, max number of blobs to process concurrently'
            --concurrent-lfs-imports [LIMIT] 'if provided, max number of LFS files to import concurrently'
            --has-globalrev                  'if provided will update globalrev'
        "#,
        )
        .arg(
            Arg::from_usage("--skip [SKIP]  'skips commits from the beginning'")
                .conflicts_with("changeset"),
        )
        .arg(
            Arg::from_usage(
                "--commits-limit [LIMIT] 'import only LIMIT first commits from revlog repo'",
            )
            .conflicts_with("changeset"),
        )
        .arg(
            Arg::with_name("fix-parent-order")
                .long("fix-parent-order")
                .value_name("FILE")
                .takes_value(true)
                .required(false)
                .help(
                    "file which fixes order or parents for commits in format 'HG_CS_ID P1_CS_ID [P2_CS_ID]'\
                     This is useful in case of merge commits - mercurial ignores order of parents of the merge commit \
                     while Mononoke doesn't ignore it. That might result in different bonsai hashes for the same \
                     Mercurial commit. Using --fix-parent-order allows to fix order of the parents."
                 )
        )
}

fn parse_fixed_parent_order<P: AsRef<Path>>(
    logger: &Logger,
    p: P,
) -> Result<HashMap<HgChangesetId, Vec<HgChangesetId>>> {
    let content = read(p)?;
    let mut res = HashMap::new();

    for line in String::from_utf8(content).map_err(Error::from)?.split("\n") {
        if line.is_empty() {
            continue;
        }
        let mut iter = line.split(" ").map(HgChangesetId::from_str).fuse();
        let maybe_hg_cs_id = iter.next();
        let hg_cs_id = match maybe_hg_cs_id {
            Some(hg_cs_id) => hg_cs_id?,
            None => {
                continue;
            }
        };

        let parents = match (iter.next(), iter.next()) {
            (Some(p1), Some(p2)) => vec![p1?, p2?],
            (Some(p), None) => {
                warn!(
                    logger,
                    "{}: parent order is fixed for a single parent, most likely won't have any effect",
                    hg_cs_id,
                );
                vec![p?]
            }
            (None, None) => {
                warn!(
                    logger, "{}: parent order is fixed for a commit with no parents, most likely won't have any effect",
                    hg_cs_id,
                );
                vec![]
            }
            (None, Some(_)) => unreachable!(),
        };
        if let Some(_) = iter.next() {
            return Err(err_msg("got 3 parents, but mercurial supports at most 2!"));
        }

        if res.insert(hg_cs_id, parents).is_some() {
            warn!(logger, "order is fixed twice for {}!", hg_cs_id);
        }
    }
    Ok(res)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let matches = setup_app().get_matches();

    args::init_cachelib(fb, &matches);
    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let revlogrepo_path = matches
        .value_of("INPUT")
        .expect("input is not specified")
        .into();

    let changeset = match matches.value_of("changeset") {
        None => None,
        Some(hash) => Some(HgNodeHash::from_str(hash)?),
    };

    let skip = if !matches.is_present("skip") {
        None
    } else {
        Some(args::get_usize(&matches, "skip", 0))
    };

    let commits_limit = if !matches.is_present("commits-limit") {
        None
    } else {
        Some(args::get_usize(&matches, "commits-limit", 0))
    };

    let no_bookmark = matches.is_present("no-bookmark");
    let prefix_bookmark = matches.value_of("prefix-bookmark");
    if no_bookmark && prefix_bookmark.is_some() {
        return Err(format_err!(
            "--no-bookmark is incompatible with --prefix-bookmark"
        ));
    }

    let bookmark_import_policy = if no_bookmark {
        blobimport_lib::BookmarkImportPolicy::Ignore
    } else {
        let prefix = match prefix_bookmark {
            Some(prefix) => AsciiString::from_ascii(prefix).unwrap(),
            None => AsciiString::new(),
        };
        blobimport_lib::BookmarkImportPolicy::Prefix(prefix)
    };

    let lfs_helper = matches.value_of("lfs-helper").map(|l| l.to_string());

    let concurrent_changesets = args::get_usize(&matches, "concurrent-changesets", 100);
    let concurrent_blobs = args::get_usize(&matches, "concurrent-blobs", 100);
    let concurrent_lfs_imports = args::get_usize(&matches, "concurrent-lfs-imports", 10);

    let phases_store = args::open_sql::<SqlPhases>(&matches);
    let globalrevs_store = args::open_sql::<SqlBonsaiGlobalrevMapping>(&matches);

    let blobrepo = if matches.is_present("no-create") {
        args::open_repo_unredacted(fb, &ctx.logger(), &matches).left_future()
    } else {
        args::create_repo_unredacted(fb, &ctx.logger(), &matches).right_future()
    };

    let fixed_parent_order = if let Some(path) = matches.value_of("fix-parent-order") {
        parse_fixed_parent_order(&logger, path)
            .context("while parsing file with fixed parent order")?
    } else {
        HashMap::new()
    };

    let has_globalrev = matches.is_present("has-globalrev");

    let blobimport = blobrepo.join3(phases_store, globalrevs_store).and_then(
        move |(blobrepo, phases_store, globalrevs_store)| {
            let phases_store = Arc::new(phases_store);
            let globalrevs_store = Arc::new(globalrevs_store);

            blobimport_lib::Blobimport {
                ctx: ctx.clone(),
                logger: ctx.logger().clone(),
                blobrepo,
                revlogrepo_path,
                changeset,
                skip,
                commits_limit,
                bookmark_import_policy,
                phases_store,
                globalrevs_store,
                lfs_helper,
                concurrent_changesets,
                concurrent_blobs,
                concurrent_lfs_imports,
                fixed_parent_order,
                has_globalrev,
            }
            .import()
            .traced(ctx.trace(), "blobimport", trace_args!())
            .map_err({
                cloned!(ctx);
                move |err| {
                    // NOTE: We log the error immediatley, then provide another one for main's
                    // Result (which will set our exit code).
                    error!(ctx.logger(), "error while blobimporting"; SlogKVError(err));
                    err_msg("blobimport exited with a failure")
                }
            })
            .then(move |result| helpers::upload_and_show_trace(ctx).then(move |_| result))
        },
    );

    let mut runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(blobimport);
    // Let the runtime finish remaining work - uploading logs etc
    runtime.shutdown_on_idle();
    result
}
