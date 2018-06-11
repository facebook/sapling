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
use futures::prelude::*;
use futures::stream::iter_ok;
use tokio_core::reactor::Core;

use blobrepo::{BlobRepo, RawNodeBlob};
use blobstore::{Blobstore, MemcacheBlobstore};
use futures_ext::{BoxFuture, FutureExt};
use manifoldblob::ManifoldBlob;
use mercurial_types::{Changeset, HgChangesetId, MPath, MPathElement, Manifest, RepositoryId};
use mercurial_types::manifest::Content;
use mononoke_types::FileContents;
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
            Arg::with_name("decode_as")
                .long("decode_as")
                .short("d")
                .takes_value(true)
                .possible_values(&["raw_node_blob"])
                .required(false)
                .help("if provided decode the value"),
        )
        .arg(
            Arg::with_name("use_memcache")
                .long("use_memcache")
                .short("m")
                .takes_value(true)
                .possible_values(&["cache_only", "no_fill", "fill_mc"])
                .required(false)
                .help("Use memcache to cache access to the blob store"),
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
            "--manifold-bucket [BUCKET] 'manifold bucket (default: mononoke_prod)' \
             --manifold-prefix [PREFIX] 'manifold prefix (default empty)' \
             --xdb-tier        [TIER]   'database tier (default: xdb.mononoke_test_2)' \
             --repo-id         [REPO_ID]'repo id (default: 0)'
             -d, --debug                'print debug level output'",
        )
        .subcommand(blobstore_fetch)
        .subcommand(content_fetch)
}

fn create_blobrepo(logger: &Logger, matches: &ArgMatches) -> BlobRepo {
    let bucket = matches
        .value_of("manifold-bucket")
        .unwrap_or("mononoke_prod");
    let prefix = matches.value_of("manifold-prefix").unwrap_or("");
    let xdb_tier = matches
        .value_of("xdb-tier")
        .unwrap_or("xdb.mononoke_test_2");
    let io_threads = 5;
    let default_cache_size = 1000000;
    BlobRepo::new_test_manifold(
        logger.clone(),
        bucket,
        prefix,
        RepositoryId::new(0),
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
    mf.lookup(&element)
        .and_then({
            let element = element.clone();
            move |entry| match entry {
                Some(entry) => Ok(entry),
                None => Err(format_err!("failed to lookup element {:?}", element)),
            }
        })
        .and_then({
            let logger = logger.clone();
            move |entry| {
                debug!(
                    logger,
                    "Fetched {:?}, hash: {:?}",
                    element,
                    entry.get_hash()
                );
                entry.get_content()
            }
        })
        .boxify()
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

    let bucket = matches
        .value_of("manifold-bucket")
        .unwrap_or("mononoke_prod");
    let prefix = matches.value_of("manifold-prefix").unwrap_or("");

    let mut core = Core::new().unwrap();
    let remote = core.remote();

    let future = match matches.subcommand() {
        (BLOBSTORE_FETCH, Some(sub_m)) => {
            let key = sub_m.value_of("KEY").unwrap().to_string();
            let decode_as = sub_m.value_of("decode_as").map(|val| val.to_string());
            let use_memcache = sub_m.value_of("use_memcache").map(|val| val.to_string());

            let blobstore = ManifoldBlob::new_with_prefix(
                bucket,
                prefix,
                vec![&remote],
                MAX_CONCURRENT_REQUESTS_PER_IO_THREAD,
            );

            match use_memcache {
                None => blobstore.get(key).boxify(),
                Some(val) => {
                    let blobstore = MemcacheBlobstore::new(blobstore);

                    if val == "cache_only" {
                        blobstore.get_memcache_only(key)
                    } else if val == "no_fill" {
                        blobstore.get_no_cache_fill(key)
                    } else {
                        blobstore.get(key)
                    }
                }
            }.map(move |value| {
                println!("{:?}", value);
                if let Some(value) = value {
                    match decode_as {
                        Some(val) => {
                            if val == "raw_node_blob" {
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

            let repo = create_blobrepo(&logger, &matches);
            fetch_content(&mut core, logger.clone(), Arc::new(repo), rev, path)
                .and_then(|content| match content {
                    Content::Executable(_) => {
                        println!("Binary file");
                        Ok(()).into_future().boxify()
                    }
                    Content::File(contents) | Content::Symlink(contents) => match contents {
                        FileContents::Bytes(bytes) => {
                            let content =
                                String::from_utf8(bytes.to_vec()).expect("non-utf8 file content");
                            println!("{}", content);
                            Ok(()).into_future().boxify()
                        }
                    },
                    Content::Tree(mf) => mf.list()
                        .collect()
                        .map(|entries| {
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
                            ()
                        })
                        .boxify(),
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
