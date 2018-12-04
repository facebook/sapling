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
extern crate promptly;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;
extern crate tokio_process;

extern crate blobrepo;
extern crate blobstore;
extern crate bonsai_utils;
extern crate bookmarks;
extern crate cmdlib;
extern crate context;
#[macro_use]
extern crate futures_ext;
extern crate manifoldblob;
extern crate mercurial_types;
extern crate mononoke_types;
extern crate reachabilityindex;
extern crate revset;
extern crate rust_thrift;
#[macro_use]
extern crate slog;
extern crate tempdir;
extern crate tokio;

mod config_repo;
mod bookmarks_manager;

use std::borrow::Borrow;
use std::collections::{BTreeMap, HashMap};

use std::fmt;
use std::io;
use std::str::FromStr;
use std::sync::Arc;

use clap::{App, Arg, SubCommand};
use failure::{err_msg, Error, Result};
use futures::future::{self, loop_fn, ok, Loop};
use futures::prelude::*;
use futures::stream::iter_ok;

use blobrepo::BlobRepo;
use blobstore::{new_memcache_blobstore, Blobstore, CacheBlobstoreExt, PrefixBlobstore};
use bonsai_utils::{bonsai_diff, BonsaiDiffResult};
use bookmarks::Bookmark;
use cmdlib::args;
use context::CoreContext;
use futures_ext::{BoxFuture, FutureExt};
use manifoldblob::ManifoldBlob;
use mercurial_types::{Changeset, HgChangesetEnvelope, HgChangesetId, HgFileEnvelope,
                      HgManifestEnvelope, HgManifestId, MPath, MPathElement, Manifest};
use mercurial_types::manifest::Content;
use mononoke_types::{BlobstoreBytes, BlobstoreValue, BonsaiChangeset, FileContents};
use reachabilityindex::{deserialize_skiplist_map, SkiplistIndex, SkiplistNodeType};
use revset::RangeNodeStream;
use rust_thrift::compact_protocol;
use slog::Logger;

const BLOBSTORE_FETCH: &'static str = "blobstore-fetch";
const BONSAI_FETCH: &'static str = "bonsai-fetch";
const CONTENT_FETCH: &'static str = "content-fetch";
const CONFIG_REPO: &'static str = "config";
const BOOKMARKS: &'static str = "bookmarks";
const SKIPLIST: &'static str = "skiplist";

const HG_CHANGESET: &'static str = "hg-changeset";
const HG_CHANGESET_DIFF: &'static str = "diff";
const HG_CHANGESET_RANGE: &'static str = "range";
const SKIPLIST_BUILD: &'static str = "build";
const SKIPLIST_READ: &'static str = "read";

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

    let bonsai_fetch = SubCommand::with_name(CONTENT_FETCH)
        .about("fetches content of the file or manifest from blobrepo")
        .args_from_usage(
            "<CHANGESET_ID>    'revision to fetch file from'
             <PATH>            'path to fetch'",
        );

    let content_fetch = SubCommand::with_name(BONSAI_FETCH)
        .about("fetches content of the file or manifest from blobrepo")
        .args_from_usage("<HG_CHANGESET_OR_BOOKMARK>    'revision to fetch file from'");

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

    let app = args::MononokeApp {
        safe_writes: false,
        hide_advanced_args: true,
        local_instances: false,
        default_glog: false,
    };
    app.build("Mononoke admin command line tool")
        .version("0.0.0")
        .about("Poke at mononoke internals for debugging and investigating data structures.")
        .subcommand(blobstore_fetch)
        .subcommand(bonsai_fetch)
        .subcommand(content_fetch)
        .subcommand(config_repo::prepare_command(SubCommand::with_name(
            CONFIG_REPO,
        )))
        .subcommand(bookmarks_manager::prepare_command(SubCommand::with_name(
            BOOKMARKS,
        )))
        .subcommand(hg_changeset)
        .subcommand(skiplist)
}

fn fetch_content_from_manifest(
    ctx: CoreContext,
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
            entry.get_content(ctx)
        }
        None => try_boxfuture!(Err(format_err!("failed to lookup element {:?}", element))),
    }
}

fn resolve_hg_rev(
    ctx: CoreContext,
    repo: &BlobRepo,
    rev: &str,
) -> impl Future<Item = HgChangesetId, Error = Error> {
    let book = Bookmark::new(&rev).unwrap();
    let hash = HgChangesetId::from_str(rev);

    repo.get_bookmark(ctx, &book).and_then({
        move |r| match r {
            Some(cs) => Ok(cs),
            None => hash,
        }
    })
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
            move |cs_id| repo.get_changeset_by_changesetid(ctx, &cs_id)
        })
        .map(|cs| cs.manifestid().clone())
        .and_then({
            cloned!(ctx, repo);
            move |root_mf_id| repo.get_manifest_by_nodeid(ctx, &root_mf_id)
        });

    let all_but_last = iter_ok::<_, Error>(path.clone().into_iter().rev().skip(1).rev());

    let folded: BoxFuture<_, Error> = mf.and_then({
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
    }).boxify();

    let basename = path.basename().clone();
    folded
        .and_then(move |mf| fetch_content_from_manifest(ctx, logger.clone(), mf, basename))
        .boxify()
}

pub fn fetch_bonsai_changeset(
    ctx: CoreContext,
    rev: &str,
    repo: &BlobRepo,
) -> impl Future<Item = BonsaiChangeset, Error = Error> {
    let hg_changeset_id = resolve_hg_rev(ctx.clone(), repo, rev);

    hg_changeset_id
        .and_then({
            cloned!(ctx, repo);
            move |hg_cs| repo.get_bonsai_from_hg(ctx, &hg_cs)
        })
        .and_then({
            let rev = rev.to_string();
            move |maybe_bonsai| maybe_bonsai.ok_or(err_msg(format!("bonsai not found for {}", rev)))
        })
        .and_then({
            cloned!(ctx, repo);
            move |bonsai| repo.get_bonsai_changeset(ctx, bonsai)
        })
}

fn get_cache<B: CacheBlobstoreExt>(
    ctx: CoreContext,
    blobstore: &B,
    key: String,
    mode: String,
) -> BoxFuture<Option<BlobstoreBytes>, Error> {
    if mode == "cache-only" {
        blobstore.get_cache_only(key)
    } else if mode == "no-fill" {
        blobstore.get_no_cache_fill(ctx, key)
    } else {
        blobstore.get(ctx, key)
    }
}

#[derive(Serialize)]
struct ChangesetDiff {
    left: HgChangesetId,
    right: HgChangesetId,
    diff: Vec<ChangesetAttrDiff>,
}

#[derive(Serialize)]
enum ChangesetAttrDiff {
    #[serde(rename = "user")] User(String, String),
    #[serde(rename = "comments")] Comments(String, String),
    #[serde(rename = "manifest")] Manifest(ManifestDiff),
    #[serde(rename = "files")] Files(Vec<String>, Vec<String>),
    #[serde(rename = "extra")] Extra(BTreeMap<String, String>, BTreeMap<String, String>),
}

#[derive(Serialize)]
struct ManifestDiff {
    modified: Vec<String>,
    deleted: Vec<String>,
}

fn mpath_to_str<P: Borrow<MPath>>(mpath: P) -> String {
    let bytes = mpath.borrow().to_vec();
    String::from_utf8_lossy(bytes.as_ref()).into_owned()
}

fn slice_to_str(slice: &[u8]) -> String {
    String::from_utf8_lossy(slice).into_owned()
}

fn hg_manifest_diff(
    ctx: CoreContext,
    repo: BlobRepo,
    left: &HgManifestId,
    right: &HgManifestId,
) -> impl Future<Item = Option<ChangesetAttrDiff>, Error = Error> {
    bonsai_diff(
        ctx,
        repo.get_root_entry(left),
        Some(repo.get_root_entry(right)),
        None,
    ).collect()
        .map(|diffs| {
            let diff = diffs.into_iter().fold(
                ManifestDiff {
                    modified: Vec::new(),
                    deleted: Vec::new(),
                },
                |mut mdiff, diff| {
                    match diff {
                        BonsaiDiffResult::Changed(path, ..)
                        | BonsaiDiffResult::ChangedReusedId(path, ..) => {
                            mdiff.modified.push(mpath_to_str(path))
                        }
                        BonsaiDiffResult::Deleted(path) => mdiff.deleted.push(mpath_to_str(path)),
                    };
                    mdiff
                },
            );
            if diff.modified.is_empty() && diff.deleted.is_empty() {
                None
            } else {
                Some(ChangesetAttrDiff::Manifest(diff))
            }
        })
}

fn hg_changeset_diff(
    ctx: CoreContext,
    repo: BlobRepo,
    left_id: &HgChangesetId,
    right_id: &HgChangesetId,
) -> impl Future<Item = ChangesetDiff, Error = Error> {
    (
        repo.get_changeset_by_changesetid(ctx.clone(), left_id),
        repo.get_changeset_by_changesetid(ctx.clone(), right_id),
    ).into_future()
        .and_then({
            cloned!(repo, left_id, right_id);
            move |(left, right)| {
                let mut diff = ChangesetDiff {
                    left: left_id,
                    right: right_id,
                    diff: Vec::new(),
                };

                if left.user() != right.user() {
                    diff.diff.push(ChangesetAttrDiff::User(
                        slice_to_str(left.user()),
                        slice_to_str(right.user()),
                    ));
                }

                if left.comments() != right.comments() {
                    diff.diff.push(ChangesetAttrDiff::Comments(
                        slice_to_str(left.comments()),
                        slice_to_str(right.comments()),
                    ))
                }

                if left.files() != right.files() {
                    diff.diff.push(ChangesetAttrDiff::Files(
                        left.files().iter().map(mpath_to_str).collect(),
                        right.files().iter().map(mpath_to_str).collect(),
                    ))
                }

                if left.extra() != right.extra() {
                    diff.diff.push(ChangesetAttrDiff::Extra(
                        left.extra()
                            .iter()
                            .map(|(k, v)| (slice_to_str(k), slice_to_str(v)))
                            .collect(),
                        right
                            .extra()
                            .iter()
                            .map(|(k, v)| (slice_to_str(k), slice_to_str(v)))
                            .collect(),
                    ))
                }

                hg_manifest_diff(ctx, repo, left.manifestid(), right.manifestid()).map(
                    move |mdiff| {
                        diff.diff.extend(mdiff);
                        diff
                    },
                )
            }
        })
}

fn build_skiplist_index<S: ToString>(
    ctx: CoreContext,
    repo: BlobRepo,
    key: S,
    logger: Logger,
) -> BoxFuture<(), Error> {
    let blobstore = repo.get_blobstore();
    let skiplist_depth = 16;
    let max_index_depth = 2000000000;
    let skiplist_index = SkiplistIndex::with_skip_edge_count(skiplist_depth);
    let cs_fetcher = repo.get_changeset_fetcher();
    let key = key.to_string();

    repo.get_bonsai_heads_maybe_stale(ctx.clone())
        .collect()
        .and_then({
            cloned!(ctx);
            move |heads| {
                loop_fn(
                    (heads.into_iter(), skiplist_index),
                    move |(mut heads, skiplist_index)| match heads.next() {
                        Some(head) => {
                            let f = skiplist_index.add_node(
                                ctx.clone(),
                                cs_fetcher.clone(),
                                head,
                                max_index_depth,
                            );

                            f.map(move |()| Loop::Continue((heads, skiplist_index)))
                                .boxify()
                        }
                        None => ok(Loop::Break(skiplist_index)).boxify(),
                    },
                )
            }
        })
        .inspect(|skiplist_index| {
            println!(
                "build {} skiplist nodes",
                skiplist_index.indexed_node_count()
            );
        })
        .map(|skiplist_index| {
            // We store only latest skip entry (i.e. entry with the longest jump)
            // This saves us storage space
            let mut thrift_merge_graph = HashMap::new();
            for (cs_id, skiplist_node_type) in skiplist_index.get_all_skip_edges() {
                let skiplist_node_type = if let SkiplistNodeType::SkipEdges(skip_edges) =
                    skiplist_node_type
                {
                    SkiplistNodeType::SkipEdges(skip_edges.last().cloned().into_iter().collect())
                } else {
                    skiplist_node_type
                };

                thrift_merge_graph.insert(cs_id.into_thrift(), skiplist_node_type.to_thrift());
            }

            compact_protocol::serialize(&thrift_merge_graph)
        })
        .and_then({
            cloned!(ctx);
            move |bytes| {
                debug!(logger, "storing {} bytes", bytes.len());
                blobstore.put(ctx, key, BlobstoreBytes::from_bytes(bytes))
            }
        })
        .boxify()
}

fn read_skiplist_index<S: ToString>(
    ctx: CoreContext,
    repo: BlobRepo,
    key: S,
    logger: Logger,
) -> BoxFuture<(), Error> {
    repo.get_blobstore()
        .get(ctx, key.to_string())
        .and_then(move |maybebytes| {
            match maybebytes {
                Some(bytes) => {
                    debug!(logger, "received {} bytes from blobstore", bytes.len());
                    let bytes = bytes.into_bytes();
                    let skiplist_map = try_boxfuture!(deserialize_skiplist_map(bytes));
                    info!(logger, "skiplist graph has {} entries", skiplist_map.len());
                }
                None => {
                    println!("not found map");
                }
            };
            ok(()).boxify()
        })
        .boxify()
}

fn main() -> Result<()> {
    let matches = setup_app().get_matches();

    let logger = args::get_logger(&matches);
    let manifold_args = args::parse_manifold_args(&matches);

    let repo_id = args::get_repo_id(&matches);

    let future = match matches.subcommand() {
        (BLOBSTORE_FETCH, Some(sub_m)) => {
            let ctx = CoreContext::test_mock();
            let key = sub_m.value_of("KEY").unwrap().to_string();
            let decode_as = sub_m.value_of("decode-as").map(|val| val.to_string());
            let use_memcache = sub_m.value_of("use-memcache").map(|val| val.to_string());
            let no_prefix = sub_m.is_present("no-prefix");

            let blobstore =
                ManifoldBlob::new_with_prefix(&manifold_args.bucket, &manifold_args.prefix);

            match (use_memcache, no_prefix) {
                (None, false) => {
                    let blobstore = PrefixBlobstore::new(blobstore, repo_id.prefix());
                    blobstore.get(ctx, key.clone()).boxify()
                }
                (None, true) => blobstore.get(ctx, key.clone()).boxify(),
                (Some(mode), false) => {
                    let blobstore = new_memcache_blobstore(
                        blobstore,
                        "manifold",
                        manifold_args.bucket.as_ref(),
                    ).unwrap();
                    let blobstore = PrefixBlobstore::new(blobstore, repo_id.prefix());
                    get_cache(ctx.clone(), &blobstore, key.clone(), mode)
                }
                (Some(mode), true) => {
                    let blobstore = new_memcache_blobstore(
                        blobstore,
                        "manifold",
                        manifold_args.bucket.as_ref(),
                    ).unwrap();
                    get_cache(ctx.clone(), &blobstore, key.clone(), mode)
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
        (BONSAI_FETCH, Some(sub_m)) => {
            let rev = sub_m.value_of("HG_CHANGESET_OR_BOOKMARK").unwrap();

            args::init_cachelib(&matches);

            // TODO(T37478150, luk) This is not a test case, fix it up in future diffs
            let ctx = CoreContext::test_mock();

            let repo = args::open_repo(ctx.clone(), &logger, &matches)?;
            fetch_bonsai_changeset(ctx, rev, repo.blobrepo())
                .map(|bcs| {
                    println!("{:?}", bcs);
                })
                .boxify()
        }
        (CONTENT_FETCH, Some(sub_m)) => {
            let rev = sub_m.value_of("CHANGESET_ID").unwrap();
            let path = sub_m.value_of("PATH").unwrap();

            args::init_cachelib(&matches);

            // TODO(T37478150, luk) This is not a test case, fix it up in future diffs
            let ctx = CoreContext::test_mock();

            let repo = args::open_repo(ctx.clone(), &logger, &matches)?;
            fetch_content(ctx, logger.clone(), repo.blobrepo(), rev, path)
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
        (CONFIG_REPO, Some(sub_m)) => {
            config_repo::handle_command(sub_m)
        }
        (BOOKMARKS, Some(sub_m)) => {
            // TODO(T37478150, luk) This is not a test case, fix it up in future diffs
            let ctx = CoreContext::test_mock();

            args::init_cachelib(&matches);
            let repo = args::open_repo(ctx.clone(), &logger, &matches)?;

            bookmarks_manager::handle_command(ctx, &repo.blobrepo(), sub_m, logger)
        }
        (HG_CHANGESET, Some(sub_m)) => match sub_m.subcommand() {
            (HG_CHANGESET_DIFF, Some(sub_m)) => {
                // TODO(T37478150, luk) This is not a test case, fix it up in future diffs
                let ctx = CoreContext::test_mock();

                let left_cs = sub_m
                    .value_of("LEFT_CS")
                    .ok_or(format_err!("LEFT_CS argument expected"))
                    .and_then(HgChangesetId::from_str);
                let right_cs = sub_m
                    .value_of("RIGHT_CS")
                    .ok_or(format_err!("RIGHT_CS argument expected"))
                    .and_then(HgChangesetId::from_str);

                args::init_cachelib(&matches);
                let repo = args::open_repo(ctx.clone(), &logger, &matches)?
                    .blobrepo()
                    .clone();

                (left_cs, right_cs)
                    .into_future()
                    .and_then(move |(left_cs, right_cs)| {
                        hg_changeset_diff(ctx, repo, &left_cs, &right_cs)
                    })
                    .and_then(|diff| {
                        serde_json::to_writer(io::stdout(), &diff)
                            .map(|_| ())
                            .map_err(Error::from)
                    })
                    .boxify()
            }
            (HG_CHANGESET_RANGE, Some(sub_m)) => {
                let start_cs = sub_m
                    .value_of("START_CS")
                    .ok_or(format_err!("START_CS argument expected"))
                    .and_then(HgChangesetId::from_str);
                let stop_cs = sub_m
                    .value_of("STOP_CS")
                    .ok_or(format_err!("STOP_CS argument expected"))
                    .and_then(HgChangesetId::from_str);

                // TODO(T37478150, luk) This is not a test case, fix it up in future diffs
                let ctx = CoreContext::test_mock();

                args::init_cachelib(&matches);
                let repo = args::open_repo(ctx.clone(), &logger, &matches)?
                    .blobrepo()
                    .clone();
                let ctx = CoreContext::test_mock();

                (start_cs, stop_cs)
                    .into_future()
                    .and_then({
                        cloned!(ctx, repo);
                        move |(start_cs, stop_cs)| {
                            (
                                repo.get_bonsai_from_hg(ctx.clone(), &start_cs),
                                repo.get_bonsai_from_hg(ctx, &stop_cs),
                            )
                        }
                    })
                    .and_then(|(start_cs_opt, stop_cs_opt)| {
                        (
                            start_cs_opt.ok_or(err_msg("failed to resolve changeset")),
                            stop_cs_opt.ok_or(err_msg("failed to resovle changeset")),
                        )
                    })
                    .and_then({
                        cloned!(repo);
                        move |(start_cs, stop_cs)| {
                            RangeNodeStream::new(
                                ctx.clone(),
                                &Arc::new(repo.clone()),
                                start_cs,
                                stop_cs,
                            ).map(move |cs| repo.get_hg_from_bonsai_changeset(ctx.clone(), cs))
                                .buffered(100)
                                .map(|cs| cs.to_hex().to_string())
                                .collect()
                        }
                    })
                    .and_then(|css| {
                        serde_json::to_writer(io::stdout(), &css)
                            .map(|_| ())
                            .map_err(Error::from)
                    })
                    .boxify()
            }
            _ => {
                println!("{}", sub_m.usage());
                ::std::process::exit(1);
            }
        },
        (SKIPLIST, Some(sub_m)) => match sub_m.subcommand() {
            (SKIPLIST_BUILD, Some(sub_m)) => {
                args::init_cachelib(&matches);
                let ctx = CoreContext::test_mock();
                let repo = args::open_repo(ctx.clone(), &logger, &matches)?
                    .blobrepo()
                    .clone();

                build_skiplist_index(ctx, repo, sub_m.value_of("BLOBSTORE_KEY").unwrap(), logger)
            }
            (SKIPLIST_READ, Some(sub_m)) => {
                args::init_cachelib(&matches);
                let ctx = CoreContext::test_mock();
                let repo = args::open_repo(ctx.clone(), &logger, &matches)?
                    .blobrepo()
                    .clone();

                read_skiplist_index(
                    ctx.clone(),
                    repo,
                    sub_m
                        .value_of("BLOBSTORE_KEY")
                        .expect("blobstore key is not specified"),
                    logger,
                )
            }
            _ => {
                println!("{}", sub_m.usage());
                ::std::process::exit(1);
            }
        },
        _ => {
            println!("{}", matches.usage());
            ::std::process::exit(1);
        }
    };

    let debug = matches.is_present("debug");

    tokio::run(future.map_err(move |err| {
        println!("{}", err);
        if debug {
            println!("\n============ DEBUG ERROR ============");
            println!("{:#?}", err);
        }
        ::std::process::exit(1);
    }));

    Ok(())
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
