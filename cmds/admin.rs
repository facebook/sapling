// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate clap;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate tokio_core;

extern crate blobrepo;
extern crate blobstore;
#[macro_use]
extern crate futures_ext;
extern crate manifoldblob;
extern crate mercurial_types;
extern crate mononoke_types;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;

use std::str::FromStr;
use std::sync::Arc;

use clap::{App, Arg, ArgMatches, SubCommand};
use failure::Error;
use futures::future;
use futures::prelude::*;
use futures::stream::iter_ok;
use tokio_core::reactor::Core;

use blobrepo::{BlobRepo, RawNodeBlob};
use blobstore::{Blobstore, MemcacheBlobstore, MemcacheBlobstoreExt, PrefixBlobstore};
use futures_ext::{BoxFuture, FutureExt};
use manifoldblob::ManifoldBlob;
use mercurial_types::{Changeset, HgChangesetId, MPath, MPathElement, Manifest, RepositoryId};
use mercurial_types::manifest::Content;
use mononoke_types::{BlobstoreBytes, FileContents};
use slog::{Drain, Level, Logger};
use slog_glog_fmt::default_drain as glog_drain;

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
                .possible_values(&["raw-node-blob"])
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

    App::new("Mononoke admin command line tool")
        .version("0.0.0")
        .about("Poke at mononoke internals for debugging and investigating data structures.")
        .args_from_usage(
            "--manifold-bucket [BUCKET] 'manifold bucket (default: mononoke_prod)'
             --manifold-prefix [PREFIX] 'manifold prefix (default empty)'
             --xdb-tier        [TIER]   'database tier (default: xdb.mononoke_test_2)'
             --repo-id         [REPO_ID]'repo id (default: 0)'
             -d, --debug                'print debug level output'",
        )
        .subcommand(blobstore_fetch)
        .subcommand(content_fetch)
}

struct ManifoldArgs<'a> {
    bucket: &'a str,
    prefix: &'a str,
    xdb_tier: &'a str,
    repo_id: RepositoryId,
}

impl<'a> ManifoldArgs<'a> {
    fn parse(matches: &'a ArgMatches) -> Self {
        let bucket = matches
            .value_of("manifold-bucket")
            .unwrap_or("mononoke_prod");
        let prefix = matches.value_of("manifold-prefix").unwrap_or("");
        let xdb_tier = matches
            .value_of("xdb-tier")
            .unwrap_or("xdb.mononoke_test_2");
        let repo_id = matches
            .value_of("repo-id")
            .map(|id: &str| id.parse::<u32>().expect("expected repo id to be a u32"))
            .unwrap_or(0);
        let repo_id = RepositoryId::new(repo_id as i32);
        Self {
            bucket,
            prefix,
            xdb_tier,
            repo_id,
        }
    }
}

fn create_blobrepo<'a>(logger: &'a Logger, manifold_args: ManifoldArgs<'a>) -> BlobRepo {
    let ManifoldArgs {
        bucket,
        prefix,
        xdb_tier,
        repo_id,
    } = manifold_args;
    let io_threads = 5;
    let default_cache_size = 1000000;
    BlobRepo::new_test_manifold(
        logger.clone(),
        bucket,
        prefix,
        repo_id,
        xdb_tier,
        default_cache_size,
        default_cache_size,
        default_cache_size,
        io_threads,
        MAX_CONCURRENT_REQUESTS_PER_IO_THREAD,
    ).expect("cannot create blobrepo")
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
    core: &mut Core,
    logger: Logger,
    repo: Arc<BlobRepo>,
    rev: &str,
    path: &str,
) -> BoxFuture<Content, Error> {
    let cs_id = try_boxfuture!(HgChangesetId::from_str(rev));
    let path = try_boxfuture!(MPath::new(path));

    let mf = repo.get_changeset_by_changesetid(&cs_id)
        .map(|cs| cs.manifestid().clone())
        .and_then(|root_mf_id| repo.get_manifest_by_nodeid(&root_mf_id.into_nodehash()));

    let all_but_last = iter_ok::<_, Error>(path.clone().into_iter().rev().skip(1).rev());

    let mf = core.run(mf).unwrap();
    let folded: BoxFuture<_, Error> = all_but_last
        .fold(mf, {
            let logger = logger.clone();
            move |mf, element| {
                fetch_content_from_manifest(logger.clone(), mf, element).and_then(|content| {
                    match content {
                        Content::Tree(mf) => Ok(mf),
                        content => Err(format_err!("expected tree entry, found {:?}", content)),
                    }
                })
            }
        })
        .boxify();

    let basename = path.basename().clone();
    folded
        .and_then(move |mf| fetch_content_from_manifest(logger.clone(), mf, basename))
        .boxify()
}

fn get_memcache<B: MemcacheBlobstoreExt>(
    blobstore: &B,
    key: String,
    mode: String,
) -> BoxFuture<Option<BlobstoreBytes>, Error> {
    if mode == "cache-only" {
        blobstore.get_memcache_only(key)
    } else if mode == "no-fill" {
        blobstore.get_no_cache_fill(key)
    } else {
        blobstore.get(key)
    }
}

fn main() {
    let matches = setup_app().get_matches();

    let logger = {
        let level = if matches.is_present("debug") {
            Level::Debug
        } else {
            Level::Info
        };

        let drain = glog_drain().filter_level(level).fuse();
        slog::Logger::root(drain, o![])
    };

    let manifold_args = ManifoldArgs::parse(&matches);

    let mut core = Core::new().unwrap();
    let remote = core.remote();

    let future = match matches.subcommand() {
        (BLOBSTORE_FETCH, Some(sub_m)) => {
            let key = sub_m.value_of("KEY").unwrap().to_string();
            let decode_as = sub_m.value_of("decode-as").map(|val| val.to_string());
            let use_memcache = sub_m.value_of("use-memcache").map(|val| val.to_string());
            let no_prefix = sub_m.is_present("no-prefix");

            let blobstore = ManifoldBlob::new_with_prefix(
                manifold_args.bucket,
                manifold_args.prefix,
                vec![&remote],
                MAX_CONCURRENT_REQUESTS_PER_IO_THREAD,
            );

            match (use_memcache, no_prefix) {
                (None, false) => {
                    let blobstore = PrefixBlobstore::new(blobstore, manifold_args.repo_id.prefix());
                    blobstore.get(key).boxify()
                }
                (None, true) => blobstore.get(key).boxify(),
                (Some(mode), false) => {
                    let blobstore = MemcacheBlobstore::new(
                        blobstore,
                        "manifold",
                        manifold_args.bucket.as_ref(),
                    ).unwrap();
                    let blobstore = PrefixBlobstore::new(blobstore, manifold_args.repo_id.prefix());
                    get_memcache(&blobstore, key, mode)
                }
                (Some(mode), true) => {
                    let blobstore = MemcacheBlobstore::new(
                        blobstore,
                        "manifold",
                        manifold_args.bucket.as_ref(),
                    ).unwrap();
                    get_memcache(&blobstore, key, mode)
                }
            }.map(move |value| {
                println!("{:?}", value);
                if let Some(value) = value {
                    match decode_as {
                        Some(val) => {
                            if val == "raw-node-blob" {
                                println!("{:?}", RawNodeBlob::deserialize(&value.into()));
                            }
                        }
                        _ => (),
                    }
                }
            })
                .boxify()
        }
        (CONTENT_FETCH, Some(sub_m)) => {
            let rev = sub_m.value_of("CHANGESET_ID").unwrap();
            let path = sub_m.value_of("PATH").unwrap();

            let repo = create_blobrepo(&logger, manifold_args);
            fetch_content(&mut core, logger.clone(), Arc::new(repo), rev, path)
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

    match core.run(future) {
        Ok(()) => (),
        Err(err) => {
            println!("{:?}", err);
            ::std::process::exit(1);
        }
    }
}
