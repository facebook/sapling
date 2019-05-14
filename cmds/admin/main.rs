// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
use std::borrow::Borrow;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::str::FromStr;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use crate::common::format_bookmark_log_entry;
use clap::{App, Arg, ArgMatches, SubCommand};
use cloned::cloned;
use failure_ext::{err_msg, format_err, Error, Result};
use futures::future::{self, loop_fn, ok, Loop};
use futures::prelude::*;
use futures::stream::iter_ok;
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use rust_thrift::compact_protocol;
use serde_derive::Serialize;

use blobrepo::BlobRepo;
use blobstore::Blobstore;
use bonsai_utils::{bonsai_diff, BonsaiDiffResult};
use bookmarks::{Bookmark, BookmarkUpdateReason, Bookmarks};
use cacheblob::{new_memcache_blobstore, CacheBlobstoreExt};
use changeset_fetcher::ChangesetFetcher;
use changesets::{ChangesetEntry, Changesets, SqlChangesets};
use cmdlib::args;
use context::CoreContext;
use dbbookmarks::SqlBookmarks;
use manifoldblob::ManifoldBlob;
use mercurial_types::manifest::Content;
use mercurial_types::{
    Changeset, HgChangesetEnvelope, HgChangesetId, HgFileEnvelope, HgManifestEnvelope,
    HgManifestId, MPath, MPathElement, Manifest,
};
use metaconfig_types::BlobConfig;
use mononoke_hg_sync_job_helper_lib::save_bundle_to_file;
use mononoke_types::{
    BlobstoreBytes, BlobstoreValue, BonsaiChangeset, ChangesetId, DateTime, FileChange,
    FileContents, Generation, RepositoryId,
};
use mutable_counters::{MutableCounters, SqlMutableCounters};
use phases::{Phase, Phases, SqlPhases};
use prefixblob::PrefixBlobstore;
use revset::RangeNodeStream;
use skiplist::{deserialize_skiplist_map, SkiplistIndex, SkiplistNodeType};
use slog::{debug, error, info, warn, Logger};

mod bookmarks_manager;
mod common;

const BLOBSTORE_FETCH: &'static str = "blobstore-fetch";
const BONSAI_FETCH: &'static str = "bonsai-fetch";
const CONTENT_FETCH: &'static str = "content-fetch";
const BOOKMARKS: &'static str = "bookmarks";
const SKIPLIST: &'static str = "skiplist";
const HASH_CONVERT: &'static str = "convert";
const HG_CHANGESET: &'static str = "hg-changeset";
const HG_CHANGESET_DIFF: &'static str = "diff";
const HG_CHANGESET_RANGE: &'static str = "range";
const HG_SYNC_BUNDLE: &'static str = "hg-sync-bundle";
const HG_SYNC_REMAINS: &'static str = "remains";
const HG_SYNC_SHOW: &'static str = "show";
const HG_SYNC_FETCH_BUNDLE: &'static str = "fetch-bundle";
const HG_SYNC_LAST_PROCESSED: &'static str = "last-processed";
const HG_SYNC_VERIFY: &'static str = "verify";
const SKIPLIST_BUILD: &'static str = "build";
const SKIPLIST_READ: &'static str = "read";
const ADD_PUBLIC_PHASES: &'static str = "add-public-phases";

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

    let bonsai_fetch = SubCommand::with_name(BONSAI_FETCH)
        .about("fetches content of the file or manifest from blobrepo")
        .args_from_usage(
            r#"<HG_CHANGESET_OR_BOOKMARK>    'revision to fetch file from'
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
                        .help("get the value of the latest processed mononoke-hg-sync counter"),
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

    let add_public_phases = SubCommand::with_name(ADD_PUBLIC_PHASES)
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
        .subcommand(bookmarks_manager::prepare_command(SubCommand::with_name(
            BOOKMARKS,
        )))
        .subcommand(hg_changeset)
        .subcommand(skiplist)
        .subcommand(convert)
        .subcommand(hg_sync)
        .subcommand(add_public_phases)
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

pub fn fetch_bonsai_changeset(
    ctx: CoreContext,
    rev: &str,
    repo: &BlobRepo,
) -> impl Future<Item = BonsaiChangeset, Error = Error> {
    let hg_changeset_id = resolve_hg_rev(ctx.clone(), repo, rev);

    hg_changeset_id
        .and_then({
            cloned!(ctx, repo);
            move |hg_cs| repo.get_bonsai_from_hg(ctx, hg_cs)
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
    #[serde(rename = "user")]
    User(String, String),
    #[serde(rename = "comments")]
    Comments(String, String),
    #[serde(rename = "manifest")]
    Manifest(ManifestDiff),
    #[serde(rename = "files")]
    Files(Vec<String>, Vec<String>),
    #[serde(rename = "extra")]
    Extra(BTreeMap<String, String>, BTreeMap<String, String>),
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
    left: HgManifestId,
    right: HgManifestId,
) -> impl Future<Item = Option<ChangesetAttrDiff>, Error = Error> {
    bonsai_diff(
        ctx,
        Box::new(repo.get_root_entry(left)),
        Some(Box::new(repo.get_root_entry(right))),
        None,
    )
    .collect()
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
    left_id: HgChangesetId,
    right_id: HgChangesetId,
) -> impl Future<Item = ChangesetDiff, Error = Error> {
    (
        repo.get_changeset_by_changesetid(ctx.clone(), left_id),
        repo.get_changeset_by_changesetid(ctx.clone(), right_id),
    )
        .into_future()
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

fn fetch_all_changesets(
    ctx: CoreContext,
    repo_id: RepositoryId,
    sqlchangesets: Arc<SqlChangesets>,
) -> impl Future<Item = Vec<ChangesetEntry>, Error = Error> {
    let num_sql_fetches = 10000;
    sqlchangesets
        .get_changesets_ids_bounds(repo_id.clone())
        .map(move |(maybe_lower_bound, maybe_upper_bound)| {
            let lower_bound = maybe_lower_bound.expect("changesets table is empty");
            let upper_bound = maybe_upper_bound.expect("changesets table is empty");
            let step = (upper_bound - lower_bound) / num_sql_fetches;
            let step = ::std::cmp::max(100, step);

            iter_ok(
                (lower_bound..upper_bound)
                    .step_by(step as usize)
                    .map(move |i| (i, i + step)),
            )
        })
        .flatten_stream()
        .and_then(move |(lower_bound, upper_bound)| {
            sqlchangesets
                .get_list_bs_cs_id_in_range(repo_id, lower_bound, upper_bound)
                .collect()
                .and_then({
                    cloned!(ctx, sqlchangesets);
                    move |ids| {
                        sqlchangesets
                            .get_many(ctx, repo_id, ids)
                            .map(|v| iter_ok(v.into_iter()))
                    }
                })
        })
        .flatten()
        .collect()
}

#[derive(Clone)]
struct InMemoryChangesetFetcher {
    fetched_changesets: Arc<HashMap<ChangesetId, ChangesetEntry>>,
    inner: Arc<dyn ChangesetFetcher>,
}

impl ChangesetFetcher for InMemoryChangesetFetcher {
    fn get_generation_number(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> BoxFuture<Generation, Error> {
        match self.fetched_changesets.get(&cs_id) {
            Some(cs_entry) => ok(Generation::new(cs_entry.gen)).boxify(),
            None => self.inner.get_generation_number(ctx, cs_id),
        }
    }

    fn get_parents(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> BoxFuture<Vec<ChangesetId>, Error> {
        match self.fetched_changesets.get(&cs_id) {
            Some(cs_entry) => ok(cs_entry.parents.clone()).boxify(),
            None => self.inner.get_parents(ctx, cs_id),
        }
    }
}

fn build_skiplist_index<S: ToString>(
    ctx: CoreContext,
    repo: BlobRepo,
    key: S,
    logger: Logger,
    sql_changesets: SqlChangesets,
) -> BoxFuture<(), Error> {
    let blobstore = repo.get_blobstore();
    // skiplist will jump up to 2^9 changesets
    let skiplist_depth = 10;
    // Index all changesets
    let max_index_depth = 20000000000;
    let skiplist_index = SkiplistIndex::with_skip_edge_count(skiplist_depth);
    let key = key.to_string();

    let cs_fetcher = fetch_all_changesets(ctx.clone(), repo.get_repoid(), Arc::new(sql_changesets))
        .map({
            let changeset_fetcher = repo.get_changeset_fetcher();
            move |fetched_changesets| {
                let fetched_changesets: HashMap<_, _> = fetched_changesets
                    .into_iter()
                    .map(|cs_entry| (cs_entry.cs_id, cs_entry))
                    .collect();
                InMemoryChangesetFetcher {
                    fetched_changesets: Arc::new(fetched_changesets),
                    inner: changeset_fetcher,
                }
            }
        });

    repo.get_bonsai_heads_maybe_stale(ctx.clone())
        .collect()
        .join(cs_fetcher)
        .and_then({
            cloned!(ctx);
            move |(heads, cs_fetcher)| {
                loop_fn(
                    (heads.into_iter(), skiplist_index),
                    move |(mut heads, skiplist_index)| match heads.next() {
                        Some(head) => {
                            let f = skiplist_index.add_node(
                                ctx.clone(),
                                Arc::new(cs_fetcher.clone()),
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
        .inspect({
            cloned!(logger);
            move |skiplist_index| {
                info!(
                    logger,
                    "build {} skiplist nodes",
                    skiplist_index.indexed_node_count()
                );
            }
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

fn add_public_phases(
    ctx: CoreContext,
    repo: BlobRepo,
    phases: Arc<SqlPhases>,
    logger: Logger,
    path: impl AsRef<str>,
    chunk_size: usize,
) -> BoxFuture<(), Error> {
    let file = try_boxfuture!(File::open(path.as_ref()).map_err(Error::from));
    let hg_changesets = BufReader::new(file).lines().filter_map(|id_str| {
        id_str
            .map_err(Error::from)
            .and_then(|v| HgChangesetId::from_str(&v))
            .ok()
    });
    let entries_processed = Arc::new(AtomicUsize::new(0));
    info!(logger, "start processing hashes");
    iter_ok(hg_changesets)
        .chunks(chunk_size)
        .and_then(move |chunk| {
            let count = chunk.len();
            repo.get_hg_bonsai_mapping(ctx.clone(), chunk)
                .map(|changesets| {
                    changesets
                        .into_iter()
                        .map(|(_, cs)| (cs, Phase::Public))
                        .collect()
                })
                .and_then({
                    cloned!(ctx, repo, phases);
                    move |phases_mapping| phases.add_all(ctx, repo, phases_mapping)
                })
                .and_then({
                    cloned!(entries_processed);
                    move |_| {
                        print!(
                            "\x1b[Khashes processed: {}\r",
                            entries_processed.fetch_add(count, Ordering::SeqCst) + count,
                        );
                        std::io::stdout().flush().expect("flush on stdout failed");
                        tokio_timer::sleep(Duration::from_secs(5)).map_err(Error::from)
                    }
                })
        })
        .for_each(|_| ok(()))
        .boxify()
}

fn process_hg_sync_verify(
    ctx: CoreContext,
    repo_id: RepositoryId,
    mutable_counters: Arc<SqlMutableCounters>,
    bookmarks: Arc<SqlBookmarks>,
    logger: Logger,
) -> BoxFuture<(), Error> {
    mutable_counters
        .get_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY)
        .map(|maybe_counter| maybe_counter.unwrap_or(0)) // See rationale under HG_SYNC_REMAINS
        .and_then({
            cloned!(ctx, repo_id);
            move |counter| {
                bookmarks.count_further_bookmark_log_entries_by_reason(
                    ctx,
                    counter as u64,
                    repo_id
                )
            }
        })
        .map({
            cloned!(repo_id, logger);
            move |counts| {
                let (
                    blobimports,
                    others
                ): (
                    Vec<(BookmarkUpdateReason, u64)>,
                    Vec<(BookmarkUpdateReason, u64)>
                ) = counts
                    .into_iter()
                    .partition(|(reason, _)| match reason {
                        BookmarkUpdateReason::Blobimport => true,
                        _ => false,
                    });

                let blobimports: u64 = blobimports
                    .into_iter()
                    .fold(0, |acc, (_, count)| acc + count);

                let others: u64 = others
                    .into_iter()
                    .fold(0, |acc, (_, count)| acc + count);

                match (blobimports > 0, others > 0) {
                    (true, true) => {
                        info!(
                            logger,
                            "Remaining bundles to replay in {:?} are not consistent: found {} blobimports and {} non-blobimports",
                            repo_id,
                            blobimports,
                            others
                        );
                    }
                    (true, false) => {
                        info!(
                            logger,
                            "All remaining bundles in {:?} are blobimports (found {})",
                            repo_id,
                            blobimports,
                        );
                    }
                    (false, true) => {
                        info!(
                            logger,
                            "All remaining bundles in {:?} are non-blobimports (found {})",
                            repo_id,
                            others,
                        );
                    }
                    (false, false) =>  {
                        info!(logger, "No replay data found in {:?}", repo_id);
                    }
                };

                ()
            }
        })
        .boxify()
}

const LATEST_REPLAYED_REQUEST_KEY: &'static str = "latest-replayed-request";

fn subcommand_process_hg_sync(
    sub_m: &ArgMatches<'_>,
    matches: &ArgMatches<'_>,
    logger: Logger,
) -> BoxFuture<(), Error> {
    let repo_id = try_boxfuture!(args::get_repo_id(&matches));

    let ctx = CoreContext::test_mock();
    let mutable_counters: Arc<_> = Arc::new(
        args::open_sql::<SqlMutableCounters>(&matches)
            .expect("Failed to open the db with mutable_counters"),
    );

    let bookmarks: Arc<_> = Arc::new(
        args::open_sql::<SqlBookmarks>(&matches).expect("Failed to open the db with bookmarks"),
    );

    match sub_m.subcommand() {
        (HG_SYNC_LAST_PROCESSED, Some(sub_m)) => match (
            sub_m.value_of("set"),
            sub_m.is_present("skip-blobimport"),
            sub_m.is_present("dry-run"),
        ) {
            (Some(..), true, ..) => {
                future::err(err_msg("cannot pass both --set and --skip-blobimport")).boxify()
            }
            (.., false, true) => future::err(err_msg(
                "--dry-run is meaningless without --skip-blobimport",
            ))
            .boxify(),
            (Some(new_value), false, false) => {
                let new_value = i64::from_str_radix(new_value, 10).unwrap();
                mutable_counters
                    .set_counter(
                        ctx.clone(),
                        repo_id,
                        LATEST_REPLAYED_REQUEST_KEY,
                        new_value,
                        None,
                    )
                    .map({
                        cloned!(repo_id, logger);
                        move |_| {
                            info!(logger, "Counter for {:?} set to {}", repo_id, new_value);
                            ()
                        }
                    })
                    .map_err({
                        cloned!(repo_id, logger);
                        move |e| {
                            info!(
                                logger,
                                "Failed to set counter for {:?} set to {}", repo_id, new_value
                            );
                            e
                        }
                    })
                    .boxify()
            }
            (None, skip, dry_run) => mutable_counters
                .get_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY)
                .and_then(move |maybe_counter| {
                    match maybe_counter {
                        None => info!(logger, "No counter found for {:?}", repo_id), //println!("No counter found for {:?}", repo_id),
                        Some(counter) => {
                            info!(logger, "Counter for {:?} has value {}", repo_id, counter)
                        }
                    };

                    match (skip, maybe_counter) {
                        (false, ..) => {
                            // We just want to log the current counter: we're done.
                            ok(()).boxify()
                        }
                        (true, None) => {
                            // We'd like to skip, but we didn't find the current counter!
                            future::err(err_msg("cannot proceed without a counter")).boxify()
                        }
                        (true, Some(counter)) => bookmarks
                            .skip_over_bookmark_log_entries_with_reason(
                                ctx.clone(),
                                counter as u64,
                                repo_id,
                                BookmarkUpdateReason::Blobimport,
                            )
                            .and_then({
                                cloned!(ctx, repo_id);
                                move |maybe_new_counter| match (maybe_new_counter, dry_run) {
                                    (Some(new_counter), true) => {
                                        info!(
                                            logger,
                                            "Counter for {:?} would be updated to {}",
                                            repo_id,
                                            new_counter
                                        );
                                        future::ok(()).boxify()
                                    }
                                    (Some(new_counter), false) => mutable_counters
                                        .set_counter(
                                            ctx.clone(),
                                            repo_id,
                                            LATEST_REPLAYED_REQUEST_KEY,
                                            new_counter as i64,
                                            Some(counter),
                                        )
                                        .and_then(move |success| match success {
                                            true => {
                                                info!(
                                                    logger,
                                                    "Counter for {:?} was updated to {}",
                                                    repo_id,
                                                    new_counter
                                                );
                                                future::ok(())
                                            }
                                            false => future::err(err_msg("update conflicted")),
                                        })
                                        .boxify(),
                                    (None, ..) => future::err(err_msg(
                                        "no valid counter position to skip ahead to",
                                    ))
                                    .boxify(),
                                }
                            })
                            .boxify(),
                    }
                })
                .boxify(),
        },
        (HG_SYNC_REMAINS, Some(sub_m)) => {
            let quiet = sub_m.is_present("quiet");
            let without_blobimport = sub_m.is_present("without-blobimport");
            mutable_counters
                .get_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY)
                .map(|maybe_counter| {
                    // yes, technically if the sync hasn't started yet
                    // and there exists a counter #0, we want return the
                    // correct value, but it's ok, since (a) there won't
                    // be a counter #0 and (b) this is just an advisory data
                    maybe_counter.unwrap_or(0)
                })
                .and_then({
                    cloned!(ctx, repo_id, without_blobimport);
                    move |counter| {
                        let counter = counter as u64;

                        let exclude_reason = match without_blobimport {
                            true => Some(BookmarkUpdateReason::Blobimport),
                            false => None,
                        };

                        bookmarks.count_further_bookmark_log_entries(
                            ctx,
                            counter,
                            repo_id,
                            exclude_reason,
                        )
                    }
                })
                .map({
                    cloned!(logger, repo_id);
                    move |remaining| {
                        if quiet {
                            println!("{}", remaining);
                        } else {
                            let name = match without_blobimport {
                                true => "non-blobimport bundles",
                                false => "bundles",
                            };

                            info!(
                                logger,
                                "Remaining {} to replay in {:?}: {}", name, repo_id, remaining
                            );
                        }
                    }
                })
                .map_err({
                    cloned!(logger, repo_id);
                    move |e| {
                        info!(
                            logger,
                            "Failed to fetch remaining bundles to replay for {:?}", repo_id
                        );
                        e
                    }
                })
                .boxify()
        }
        (HG_SYNC_SHOW, Some(sub_m)) => {
            let limit = args::get_u64(sub_m, "limit", 10);
            args::init_cachelib(&matches);
            let repo_fut = args::open_repo(&logger, &matches);

            let current_counter = mutable_counters
                .get_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY)
                .map(|maybe_counter| {
                    // yes, technically if the sync hasn't started yet
                    // and there exists a counter #0, we want return the
                    // correct value, but it's ok, since (a) there won't
                    // be a counter #0 and (b) this is just an advisory data
                    maybe_counter.unwrap_or(0)
                });

            repo_fut.and_then(move |repo| {
                current_counter.map({
                    cloned!(ctx);
                    move |id| {
                    bookmarks.read_next_bookmark_log_entries(ctx.clone(), id as u64, repo_id, limit)
                }})
                .flatten_stream()
                .and_then({
                    cloned!(ctx);
                    move |entry| {
                        let bundle_id = entry.id;
                        match entry.to_changeset_id {
                            Some(bcs_id) => repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                                .map(|hg_cs_id| format!("{}", hg_cs_id)).left_future(),
                            None => future::ok("DELETED".to_string()).right_future()
                        }.map(move |hg_cs_id| {
                            format_bookmark_log_entry(
                                false, /* json_flag */
                                hg_cs_id,
                                entry.reason,
                                entry.timestamp,
                                "hg",
                                entry.bookmark_name,
                                Some(bundle_id),
                            )
                        })
                    }
                })
                .for_each(|s| {
                    println!("{}", s);
                    Ok(())
                })
            })
            .boxify()
        }
        (HG_SYNC_FETCH_BUNDLE, Some(sub_m)) => {
            args::init_cachelib(&matches);
            let repo_fut = args::open_repo(&logger, &matches);
            let id = args::get_u64_opt(sub_m, "id");
            let id = try_boxfuture!(id.ok_or(err_msg("--id is not specified")));
            if id == 0 {
                return future::err(err_msg("--id has to be greater than 0")).boxify();
            }

            let output_file = try_boxfuture!(sub_m
                .value_of("output-file")
                .ok_or(err_msg("--output-file is not specified"))
                .map(std::path::PathBuf::from));

            bookmarks
                .read_next_bookmark_log_entries(ctx.clone(), id - 1, repo_id, 1)
                .into_future()
                .map(|(entry, _)| entry)
                .map_err(|(err, _)| err)
                .and_then(move |maybe_log_entry| {
                    let log_entry =
                        try_boxfuture!(maybe_log_entry.ok_or(err_msg("no log entries found")));
                    if log_entry.id != id as i64 {
                        return future::err(err_msg("no entry with specified id found")).boxify();
                    }
                    let bundle_replay_data = try_boxfuture!(log_entry
                        .reason
                        .get_bundle_replay_data()
                        .ok_or(err_msg("no bundle found")));
                    let bundle_handle = bundle_replay_data.bundle_handle.clone();

                    repo_fut
                        .and_then(move |repo| {
                            save_bundle_to_file(
                                ctx,
                                repo.get_blobstore(),
                                &bundle_handle,
                                output_file,
                                true, /* create */
                            )
                        })
                        .boxify()
                })
                .boxify()
        }
        (HG_SYNC_VERIFY, Some(..)) => {
            process_hg_sync_verify(ctx, repo_id, mutable_counters, bookmarks, logger)
        }
        _ => {
            eprintln!("{}", matches.usage());
            ::std::process::exit(1);
        }
    }
}

fn subcommand_blobstore_fetch(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    let blobstore_args = args::parse_blobstore_args(&matches);
    let repo_id = try_boxfuture!(args::get_repo_id(&matches));

    let (bucket, prefix) = match blobstore_args {
        BlobConfig::Manifold { bucket, prefix } => (bucket, prefix),
        bad => panic!("Unsupported blobstore: {:#?}", bad),
    };

    let ctx = CoreContext::test_mock();
    let key = sub_m.value_of("KEY").unwrap().to_string();
    let decode_as = sub_m.value_of("decode-as").map(|val| val.to_string());
    let use_memcache = sub_m.value_of("use-memcache").map(|val| val.to_string());
    let no_prefix = sub_m.is_present("no-prefix");

    let blobstore = ManifoldBlob::new_with_prefix(&bucket, &prefix);

    match (use_memcache, no_prefix) {
        (None, false) => {
            let blobstore = PrefixBlobstore::new(blobstore, repo_id.prefix());
            blobstore.get(ctx, key.clone()).boxify()
        }
        (None, true) => blobstore.get(ctx, key.clone()).boxify(),
        (Some(mode), false) => {
            let blobstore = new_memcache_blobstore(blobstore, "manifold", bucket).unwrap();
            let blobstore = PrefixBlobstore::new(blobstore, repo_id.prefix());
            get_cache(ctx.clone(), &blobstore, key.clone(), mode)
        }
        (Some(mode), true) => {
            let blobstore = new_memcache_blobstore(blobstore, "manifold", bucket).unwrap();
            get_cache(ctx.clone(), &blobstore, key.clone(), mode)
        }
    }
    .map(move |value| {
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

fn subcommand_bonsai_fetch(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    let rev = sub_m
        .value_of("HG_CHANGESET_OR_BOOKMARK")
        .unwrap()
        .to_string();

    args::init_cachelib(&matches);

    // TODO(T37478150, luk) This is not a test case, fix it up in future diffs
    let ctx = CoreContext::test_mock();
    let json_flag = sub_m.is_present("json");

    args::open_repo(&logger, &matches)
        .and_then(move |blobrepo| fetch_bonsai_changeset(ctx, &rev, &blobrepo))
        .map(move |bcs| {
            if json_flag {
                match serde_json::to_string(&SerializableBonsaiChangeset::from(bcs)) {
                    Ok(json) => println!("{}", json),
                    Err(e) => println!("{}", e),
                }
            } else {
                println!(
                    "BonsaiChangesetId: {} \n\
                     Author: {} \n\
                     Message: {} \n\
                     FileChanges:",
                    bcs.get_changeset_id(),
                    bcs.author(),
                    bcs.message().lines().next().unwrap_or("")
                );

                for (path, file_change) in bcs.file_changes() {
                    match file_change {
                        Some(file_change) => match file_change.copy_from() {
                            Some(_) => {
                                println!("\t COPY/MOVE: {} {}", path, file_change.content_id())
                            }
                            None => {
                                println!("\t ADDED/MODIFIED: {} {}", path, file_change.content_id())
                            }
                        },
                        None => println!("\t REMOVED: {}", path),
                    }
                }
            }
        })
        .boxify()
}

fn subcommand_content_fetch(
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

fn subcommand_hg_changeset(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    match sub_m.subcommand() {
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
            args::open_repo(&logger, &matches)
                .and_then(move |repo| {
                    (left_cs, right_cs)
                        .into_future()
                        .and_then(move |(left_cs, right_cs)| {
                            hg_changeset_diff(ctx, repo, left_cs, right_cs)
                        })
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
            args::open_repo(&logger, &matches)
                .and_then(move |repo| {
                    (start_cs, stop_cs)
                        .into_future()
                        .and_then({
                            cloned!(ctx, repo);
                            move |(start_cs, stop_cs)| {
                                (
                                    repo.get_bonsai_from_hg(ctx.clone(), start_cs),
                                    repo.get_bonsai_from_hg(ctx, stop_cs),
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
                                    repo.get_changeset_fetcher(),
                                    start_cs,
                                    stop_cs,
                                )
                                .map(move |cs| repo.get_hg_from_bonsai_changeset(ctx.clone(), cs))
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
                })
                .boxify()
        }
        _ => {
            println!("{}", sub_m.usage());
            ::std::process::exit(1);
        }
    }
}

fn subcommand_skiplist(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    match sub_m.subcommand() {
        (SKIPLIST_BUILD, Some(sub_m)) => {
            let key = sub_m
                .value_of("BLOBSTORE_KEY")
                .expect("blobstore key is not specified")
                .to_string();

            args::init_cachelib(&matches);
            let ctx = CoreContext::test_mock();
            let sql_changesets = args::open_sql::<SqlChangesets>(&matches);
            let repo = args::open_repo(&logger, &matches);
            repo.join(sql_changesets)
                .and_then(move |(repo, sql_changesets)| {
                    build_skiplist_index(ctx, repo, key, logger, sql_changesets)
                })
                .boxify()
        }
        (SKIPLIST_READ, Some(sub_m)) => {
            let key = sub_m
                .value_of("BLOBSTORE_KEY")
                .expect("blobstore key is not specified")
                .to_string();

            args::init_cachelib(&matches);
            let ctx = CoreContext::test_mock();
            args::open_repo(&logger, &matches)
                .and_then(move |repo| read_skiplist_index(ctx.clone(), repo, key, logger))
                .boxify()
        }
        _ => {
            println!("{}", sub_m.usage());
            ::std::process::exit(1);
        }
    }
}

fn subcommand_hash_convert(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    let source_hash = sub_m.value_of("HASH").unwrap().to_string();
    let source = sub_m.value_of("from").unwrap().to_string();
    let target = sub_m.value_of("to").unwrap();
    // Check that source and target are different types.
    assert_eq!(
        false,
        (source == "hg") ^ (target == "bonsai"),
        "source and target should be different"
    );
    args::init_cachelib(&matches);
    // TODO(T37478150, luk) This is not a test case, fix it up in future diffs
    let ctx = CoreContext::test_mock();
    args::open_repo(&logger, &matches)
        .and_then(move |repo| {
            if source == "hg" {
                repo.get_bonsai_from_hg(
                    ctx,
                    HgChangesetId::from_str(&source_hash)
                        .expect("source hash is not valid hg changeset id"),
                )
                .and_then(move |maybebonsai| {
                    match maybebonsai {
                        Some(bonsai) => {
                            println!("{}", bonsai);
                        }
                        None => {
                            panic!("no matching mononoke id found");
                        }
                    }
                    Ok(())
                })
                .left_future()
            } else {
                repo.get_hg_from_bonsai_changeset(
                    ctx,
                    ChangesetId::from_str(&source_hash)
                        .expect("source hash is not valid mononoke id"),
                )
                .and_then(move |mercurial| {
                    println!("{}", mercurial);
                    Ok(())
                })
                .right_future()
            }
        })
        .boxify()
}

fn subcommand_add_public_phases(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    let path = String::from(sub_m.value_of("input-file").unwrap());
    let chunk_size = sub_m
        .value_of("chunk-size")
        .and_then(|chunk_size| chunk_size.parse::<usize>().ok())
        .unwrap_or(16384);
    let ctx = CoreContext::test_mock();
    args::init_cachelib(&matches);
    let phases: Arc<_> =
        Arc::new(args::open_sql::<SqlPhases>(&matches).expect("Failed to open the db with phases"));
    args::open_repo(&logger, &matches)
        .and_then(move |repo| add_public_phases(ctx, repo, phases, logger, path, chunk_size))
        .boxify()
}

fn main() -> Result<()> {
    let matches = setup_app().get_matches();

    let logger = args::get_logger(&matches);
    let error_logger = logger.clone();

    let future = match matches.subcommand() {
        (BLOBSTORE_FETCH, Some(sub_m)) => subcommand_blobstore_fetch(logger, &matches, sub_m),
        (BONSAI_FETCH, Some(sub_m)) => subcommand_bonsai_fetch(logger, &matches, sub_m),
        (CONTENT_FETCH, Some(sub_m)) => subcommand_content_fetch(logger, &matches, sub_m),
        (BOOKMARKS, Some(sub_m)) => {
            args::init_cachelib(&matches);
            // TODO(T37478150, luk) This is not a test case, fix it up in future diffs
            let ctx = CoreContext::test_mock();
            let repo_fut = args::open_repo(&logger, &matches).boxify();
            bookmarks_manager::handle_command(ctx, repo_fut, sub_m, logger)
        }
        (HG_CHANGESET, Some(sub_m)) => subcommand_hg_changeset(logger, &matches, sub_m),
        (HG_SYNC_BUNDLE, Some(sub_m)) => {
            subcommand_process_hg_sync(sub_m, &matches, logger.clone())
        }
        (SKIPLIST, Some(sub_m)) => subcommand_skiplist(logger, &matches, sub_m),
        (HASH_CONVERT, Some(sub_m)) => subcommand_hash_convert(logger, &matches, sub_m),
        (ADD_PUBLIC_PHASES, Some(sub_m)) => subcommand_add_public_phases(logger, &matches, sub_m),
        _ => {
            eprintln!("{}", matches.usage());
            ::std::process::exit(1);
        }
    };

    let debug = matches.is_present("debug");

    tokio::run(future.map_err(move |err| {
        error!(error_logger, "{:?}", err);
        if debug {
            error!(error_logger, "\n============ DEBUG ERROR ============");
            error!(error_logger, "{:#?}", err);
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

#[derive(Serialize)]
pub struct SerializableBonsaiChangeset {
    pub parents: Vec<ChangesetId>,
    pub author: String,
    pub author_date: DateTime,
    pub committer: Option<String>,
    // XXX should committer date always be recorded? If so, it should probably be a
    // monotonically increasing value:
    // max(author date, max(committer date of parents) + epsilon)
    pub committer_date: Option<DateTime>,
    pub message: String,
    pub extra: BTreeMap<String, Vec<u8>>,
    pub file_changes: BTreeMap<String, Option<FileChange>>,
}

impl From<BonsaiChangeset> for SerializableBonsaiChangeset {
    fn from(bonsai: BonsaiChangeset) -> Self {
        let mut parents = Vec::new();
        parents.extend(bonsai.parents());

        let author = bonsai.author().to_string();
        let author_date = bonsai.author_date().clone();

        let committer = bonsai.committer().map(|s| s.to_string());
        let committer_date = bonsai.committer_date().cloned();

        let message = bonsai.message().to_string();

        let extra = bonsai
            .extra()
            .map(|(k, v)| (k.to_string(), v.to_vec()))
            .collect();

        let file_changes = bonsai
            .file_changes()
            .map(|(k, v)| {
                (
                    String::from_utf8(k.to_vec()).expect("Found invalid UTF-8"),
                    v.cloned(),
                )
            })
            .collect();

        SerializableBonsaiChangeset {
            parents,
            author,
            author_date,
            committer,
            committer_date,
            message,
            extra,
            file_changes,
        }
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
