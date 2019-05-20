// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use clap::ArgMatches;
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use failure_ext::{format_err, Error};
use futures::future;
use futures::prelude::*;
use futures::stream::iter_ok;
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use mercurial_types::manifest::Content;
use mercurial_types::{Changeset, MPath, MPathElement, Manifest};
use mononoke_types::FileContents;
use slog::{debug, Logger};

use crate::common::resolve_hg_rev;

pub fn subcommand_content_fetch(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    let rev = sub_m.value_of("CHANGESET_ID").unwrap().to_string();
    let path = sub_m.value_of("PATH").unwrap().to_string();

    args::init_cachelib(&matches);

    // TODO(T37478150, luk) This is not a test case, fix it up in future diffs
    let ctx = CoreContext::test_mock();

    args::open_repo(&logger, &matches)
        .and_then(move |blobrepo| fetch_content(ctx, logger.clone(), &blobrepo, &rev, &path))
        .and_then(|content| {
            match content {
                Content::Executable(_) => {
                    println!("Binary file");
                }
                Content::File(contents) | Content::Symlink(contents) => match contents {
                    FileContents::Bytes(bytes) => {
                        let content =
                            String::from_utf8(bytes.to_vec()).expect("non-utf8 file content");
                        println!("{}", content);
                    }
                },
                Content::Tree(mf) => {
                    let entries: Vec<_> = mf.list().collect();
                    let mut longest_len = 0;
                    for entry in entries.iter() {
                        let basename_len =
                            entry.get_name().map(|basename| basename.len()).unwrap_or(0);
                        if basename_len > longest_len {
                            longest_len = basename_len;
                        }
                    }
                    for entry in entries {
                        let mut basename = String::from_utf8_lossy(
                            entry.get_name().expect("empty basename found").as_bytes(),
                        )
                        .to_string();
                        for _ in basename.len()..longest_len {
                            basename.push(' ');
                        }
                        println!("{} {} {:?}", basename, entry.get_hash(), entry.get_type());
                    }
                }
            }
            future::ok(()).boxify()
        })
        .boxify()
}

fn fetch_content_from_manifest(
    ctx: CoreContext,
    logger: Logger,
    mf: Box<dyn Manifest + Sync>,
    element: MPathElement,
) -> BoxFuture<Content, Error> {
    match mf.lookup(&element) {
        Some(entry) => {
            debug!(
                logger,
                "Fetched {:?}, hash: {:?}",
                element,
                entry.get_hash()
            );
            entry.get_content(ctx)
        }
        None => try_boxfuture!(Err(format_err!("failed to lookup element {:?}", element))),
    }
}

fn fetch_content(
    ctx: CoreContext,
    logger: Logger,
    repo: &BlobRepo,
    rev: &str,
    path: &str,
) -> BoxFuture<Content, Error> {
    let path = try_boxfuture!(MPath::new(path));
    let resolved_cs_id = resolve_hg_rev(ctx.clone(), repo, rev);

    let mf = resolved_cs_id
        .and_then({
            cloned!(ctx, repo);
            move |cs_id| repo.get_changeset_by_changesetid(ctx, cs_id)
        })
        .map(|cs| cs.manifestid().clone())
        .and_then({
            cloned!(ctx, repo);
            move |root_mf_id| repo.get_manifest_by_nodeid(ctx, root_mf_id)
        });

    let all_but_last = iter_ok::<_, Error>(path.clone().into_iter().rev().skip(1).rev());

    let folded: BoxFuture<_, Error> = mf
        .and_then({
            cloned!(ctx, logger);
            move |mf| {
                all_but_last.fold(mf, move |mf, element| {
                    fetch_content_from_manifest(ctx.clone(), logger.clone(), mf, element).and_then(
                        |content| match content {
                            Content::Tree(mf) => Ok(mf),
                            content => Err(format_err!("expected tree entry, found {:?}", content)),
                        },
                    )
                })
            }
        })
        .boxify();

    let basename = path.basename().clone();
    folded
        .and_then(move |mf| fetch_content_from_manifest(ctx, logger.clone(), mf, basename))
        .boxify()
}
