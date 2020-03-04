/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bytes::BytesMut;
use clap::ArgMatches;
use cloned::cloned;
use cmdlib::{args, helpers};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use futures_old::future;
use futures_old::prelude::*;
use futures_old::stream::iter_ok;
use mercurial_types::manifest::Content;
use mercurial_types::{HgManifest, MPath, MPathElement};
use slog::{debug, Logger};

use crate::error::SubcommandError;

pub async fn subcommand_content_fetch<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a ArgMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let rev = sub_m.value_of("CHANGESET_ID").unwrap().to_string();
    let path = sub_m.value_of("PATH").unwrap().to_string();

    args::init_cachelib(fb, &matches, None);

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    args::open_repo(fb, &logger, &matches)
        .and_then(move |blobrepo| fetch_content(ctx, logger.clone(), &blobrepo, &rev, &path))
        .and_then(|content| match content {
            Content::Executable(_) => {
                println!("Binary file");
                future::ok(()).boxify()
            }
            Content::File(stream) | Content::Symlink(stream) => stream
                .fold(BytesMut::new(), |mut buff, file_bytes| {
                    buff.extend_from_slice(file_bytes.as_bytes().as_ref());
                    Result::<_, Error>::Ok(buff)
                })
                .map(|bytes| {
                    let content = String::from_utf8(bytes.to_vec()).expect("non-utf8 file content");
                    println!("{}", content);
                })
                .boxify(),
            Content::Tree(mf) => {
                let entries: Vec<_> = mf.list().collect();
                let mut longest_len = 0;
                for entry in entries.iter() {
                    let basename_len = entry.get_name().map(|basename| basename.len()).unwrap_or(0);
                    if basename_len > longest_len {
                        longest_len = basename_len;
                    }
                }
                for entry in entries {
                    let mut basename = String::from_utf8_lossy(
                        entry.get_name().expect("empty basename found").as_ref(),
                    )
                    .to_string();
                    for _ in basename.len()..longest_len {
                        basename.push(' ');
                    }
                    println!("{} {} {:?}", basename, entry.get_hash(), entry.get_type());
                }
                future::ok(()).boxify()
            }
        })
        .map_err(|e| e.into())
        .compat()
        .await
}

fn fetch_content_from_manifest(
    ctx: CoreContext,
    logger: Logger,
    mf: Box<dyn HgManifest + Sync>,
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
    let resolved_hg_cs_id = helpers::csid_resolve(ctx.clone(), repo.clone(), rev.to_string())
        .and_then({
            cloned!(ctx, repo);
            move |bcs_id| repo.get_hg_from_bonsai_changeset(ctx, bcs_id)
        });

    let mf = resolved_hg_cs_id
        .and_then({
            cloned!(ctx, repo);
            move |cs_id| cs_id.load(ctx, repo.blobstore()).from_err()
        })
        .map(|cs| cs.manifestid().clone())
        .and_then({
            cloned!(ctx, repo);
            move |root_mf_id| {
                root_mf_id
                    .load(ctx, repo.blobstore())
                    .from_err()
                    .map(|mf| Box::new(mf) as Box<dyn HgManifest + Sync>)
            }
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
