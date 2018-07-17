// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate clap;
#[macro_use]
extern crate cloned;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;

extern crate blobrepo;
extern crate blobstore;
extern crate cmdlib;
#[macro_use]
extern crate futures_ext;
extern crate manifoldblob;
extern crate mercurial_types;
extern crate mononoke_types;
#[macro_use]
extern crate slog;
extern crate tokio;

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use clap::{App, Arg, SubCommand};
use failure::{Error, Result};
use futures::future;
use futures::prelude::*;
use futures::stream::iter_ok;

use blobrepo::BlobRepo;
use blobstore::{new_memcache_blobstore, Blobstore, CacheBlobstoreExt, PrefixBlobstore};
use cmdlib::args;
use futures_ext::{BoxFuture, FutureExt};
use manifoldblob::ManifoldBlob;
use mercurial_types::{Changeset, HgChangesetEnvelope, HgChangesetId, HgFileEnvelope,
                      HgManifestEnvelope, MPath, MPathElement, Manifest};
use mercurial_types::manifest::Content;
use mononoke_types::{BlobstoreBytes, BlobstoreValue, FileContents};
use slog::Logger;

const BLOBSTORE_FETCH: &'static str = "blobstore-fetch";
const CONTENT_FETCH: &'static str = "content-fetch";
const MAX_CONCURRENT_REQUESTS_PER_IO_THREAD: usize = 4;

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
        );

    let content_fetch = SubCommand::with_name(CONTENT_FETCH)
        .about("fetches content of the file or manifest from blobrepo")
        .args_from_usage(
            "<CHANGESET_ID>    'revision to fetch file from'
             <PATH>            'path to fetch'",
        );

    let app = args::MononokeApp {
        safe_writes: false,
        hide_advanced_args: true,
        local_instances: false,
    };
    app.build("Mononoke admin command line tool")
        .version("0.0.0")
        .about("Poke at mononoke internals for debugging and investigating data structures.")
        .subcommand(blobstore_fetch)
        .subcommand(content_fetch)
}

fn fetch_content_from_manifest(
    logger: Logger,
    mf: Box<Manifest + Sync>,
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
            entry.get_content()
        }
        None => try_boxfuture!(Err(format_err!("failed to lookup element {:?}", element))),
    }
}

fn fetch_content(
    logger: Logger,
    repo: Arc<BlobRepo>,
    rev: &str,
    path: &str,
) -> BoxFuture<Content, Error> {
    let cs_id = try_boxfuture!(HgChangesetId::from_str(rev));
    let path = try_boxfuture!(MPath::new(path));

    let mf = repo.get_changeset_by_changesetid(&cs_id)
        .map(|cs| cs.manifestid().clone())
        .and_then({
            cloned!(repo);
            move |root_mf_id| repo.get_manifest_by_nodeid(&root_mf_id.into_nodehash())
        });

    let all_but_last = iter_ok::<_, Error>(path.clone().into_iter().rev().skip(1).rev());

    let folded: BoxFuture<_, Error> = mf.and_then({
        cloned!(logger);
        move |mf| {
            all_but_last.fold(mf, move |mf, element| {
                fetch_content_from_manifest(logger.clone(), mf, element).and_then(|content| {
                    match content {
                        Content::Tree(mf) => Ok(mf),
                        content => Err(format_err!("expected tree entry, found {:?}", content)),
                    }
                })
            })
        }
    }).boxify();

    let basename = path.basename().clone();
    folded
        .and_then(move |mf| fetch_content_from_manifest(logger.clone(), mf, basename))
        .boxify()
}

fn get_cache<B: CacheBlobstoreExt>(
    blobstore: &B,
    key: String,
    mode: String,
) -> BoxFuture<Option<BlobstoreBytes>, Error> {
    if mode == "cache-only" {
        blobstore.get_cache_only(key)
    } else if mode == "no-fill" {
        blobstore.get_no_cache_fill(key)
    } else {
        blobstore.get(key)
    }
}

fn main() {
    let matches = setup_app().get_matches();

    let logger = args::get_logger(&matches);
    let manifold_args = args::parse_manifold_args(&matches, 1_000_000);

    let repo_id = args::get_repo_id(&matches);

    let future = match matches.subcommand() {
        (BLOBSTORE_FETCH, Some(sub_m)) => {
            let key = sub_m.value_of("KEY").unwrap().to_string();
            let decode_as = sub_m.value_of("decode-as").map(|val| val.to_string());
            let use_memcache = sub_m.value_of("use-memcache").map(|val| val.to_string());
            let no_prefix = sub_m.is_present("no-prefix");

            let blobstore = ManifoldBlob::new_with_prefix(
                &manifold_args.bucket,
                &manifold_args.prefix,
                MAX_CONCURRENT_REQUESTS_PER_IO_THREAD,
            );

            match (use_memcache, no_prefix) {
                (None, false) => {
                    let blobstore = PrefixBlobstore::new(blobstore, repo_id.prefix());
                    blobstore.get(key.clone()).boxify()
                }
                (None, true) => blobstore.get(key.clone()).boxify(),
                (Some(mode), false) => {
                    let blobstore = new_memcache_blobstore(
                        blobstore,
                        "manifold",
                        manifold_args.bucket.as_ref(),
                    ).unwrap();
                    let blobstore = PrefixBlobstore::new(blobstore, repo_id.prefix());
                    get_cache(&blobstore, key.clone(), mode)
                }
                (Some(mode), true) => {
                    let blobstore = new_memcache_blobstore(
                        blobstore,
                        "manifold",
                        manifold_args.bucket.as_ref(),
                    ).unwrap();
                    get_cache(&blobstore, key.clone(), mode)
                }
            }.map(move |value| {
                println!("{:?}", value);
                if let Some(value) = value {
                    let decode_as = decode_as.as_ref().and_then(|val| {
                        let val = val.as_str();
                        if val == "auto" {
                            detect_decode(&key, &logger)
                        } else {
                            Some(val)
                        }
                    });

                    match decode_as {
                        Some("changeset") => display(&HgChangesetEnvelope::from_blob(value.into())),
                        Some("manifest") => display(&HgManifestEnvelope::from_blob(value.into())),
                        Some("file") => display(&HgFileEnvelope::from_blob(value.into())),
                        // TODO: (rain1) T30974137 add a better way to print out file contents
                        Some("contents") => println!("{:?}", FileContents::from_blob(value.into())),
                        _ => (),
                    }
                }
            })
                .boxify()
        }
        (CONTENT_FETCH, Some(sub_m)) => {
            let rev = sub_m.value_of("CHANGESET_ID").unwrap();
            let path = sub_m.value_of("PATH").unwrap();

            let repo = args::open_blobrepo(&logger, &matches);
            fetch_content(logger.clone(), Arc::new(repo), rev, path)
                .and_then(|content| {
                    match content {
                        Content::Executable(_) => {
                            println!("Binary file");
                        }
                        Content::File(contents) | Content::Symlink(contents) => match contents {
                            FileContents::Bytes(bytes) => {
                                let content = String::from_utf8(bytes.to_vec())
                                    .expect("non-utf8 file content");
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
                                ).to_string();
                                for _ in basename.len()..longest_len {
                                    basename.push(' ');
                                }
                                println!(
                                    "{} {} {:?}",
                                    basename,
                                    entry.get_hash(),
                                    entry.get_type()
                                );
                            }
                        }
                    }
                    future::ok(()).boxify()
                })
                .boxify()
        }
        _ => {
            println!("{}", matches.usage());
            ::std::process::exit(1);
        }
    };

    tokio::run(future.map_err(|err| {
        println!("{:?}", err);
        ::std::process::exit(1);
    }));
}

fn detect_decode(key: &str, logger: &Logger) -> Option<&'static str> {
    // Use a simple heuristic to figure out how to decode this key.
    if key.find("hgchangeset.").is_some() {
        info!(logger, "Detected changeset key");
        Some("changeset")
    } else if key.find("hgmanifest.").is_some() {
        info!(logger, "Detected manifest key");
        Some("manifest")
    } else if key.find("hgfilenode.").is_some() {
        info!(logger, "Detected file key");
        Some("file")
    } else if key.find("content.").is_some() {
        info!(logger, "Detected content key");
        Some("contents")
    } else {
        warn!(
            logger,
            "Unable to detect how to decode this blob based on key";
            "key" => key,
        );
        None
    }
}

fn display<T>(res: &Result<T>)
where
    T: fmt::Display + fmt::Debug,
{
    match res {
        Ok(val) => println!("---\n{}---", val),
        err => println!("{:?}", err),
    }
}
