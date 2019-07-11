// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::errors::*;
use crate::mononoke_repo::{MononokeRepo, SqlStreamingCloneConfig};
use blobrepo::BlobRepo;
use blobrepo::HgBlobChangeset;
use bookmarks::{Bookmark, BookmarkName, BookmarkPrefix};
use bundle2_resolver;
use bytes::{BufMut, Bytes, BytesMut};
use cloned::cloned;
use context::CoreContext;
use failure::{err_msg, format_err};
use fbwhoami::FbWhoAmI;
use futures::future::ok;
use futures::{future, stream, try_ready, Async, Future, IntoFuture, Poll, Stream};
use futures_ext::{
    select_all, try_boxfuture, try_boxstream, BoxFuture, BoxStream, BufferedParams, FutureExt,
    StreamExt, StreamTimeoutError,
};
use futures_stats::{StreamStats, Timed, TimedStreamTrait};
use hgproto::{self, GetbundleArgs, GettreepackArgs, HgCommandRes, HgCommands};
use hooks::HookManager;
use itertools::Itertools;
use maplit::hashmap;
use mercurial_bundles::{create_bundle_stream, parts, wirepack, Bundle2Item};
use mercurial_types::manifest_utils::{
    changed_entry_stream_with_pruner, CombinatorPruner, DeletedPruner, EntryStatus, FilePruner,
    Pruner,
};
use mercurial_types::{
    convert_parents_to_remotefilelog_format, percent_encode, Delta, Entry, HgBlobNode,
    HgChangesetId, HgFileNodeId, HgManifestId, MPath, RepoPath, Type, NULL_CSID, NULL_HASH,
};
use metaconfig_types::RepoReadOnly;
use percent_encoding;
use phases::Phases;
use rand::{self, Rng};
use reachabilityindex::LeastCommonAncestorsHint;
use remotefilelog::{create_remotefilelog_blob, get_unordered_file_history_for_multiple_nodes};
use scribe::ScribeClient;
use scuba_ext::{ScribeClientImplementation, ScubaSampleBuilder, ScubaSampleBuilderExt};
use serde_json::{self, json};
use slog::{debug, info, o};
use stats::{define_stats, Histogram, Timeseries};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::iter::FromIterator;
use std::mem;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use streaming_clone::RevlogStreamingChunks;
use time_ext::DurationExt;
use tokio::timer::timeout::Error as TimeoutError;
use tokio::util::FutureExt as TokioFutureExt;
use tracing::{trace_args, Traced};

const MAX_NODES_TO_LOG: usize = 5;

define_stats! {
    prefix = "mononoke.repo_client";
    getbundle_ms:
        histogram(500, 0, 10_000, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    gettreepack_ms:
        histogram(500, 0, 20_000, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    getfiles_ms:
        histogram(500, 0, 20_000, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    total_tree_count: timeseries(RATE, SUM),
    quicksand_tree_count: timeseries(RATE, SUM),
    total_tree_size: timeseries(RATE, SUM),
    quicksand_tree_size: timeseries(RATE, SUM),
    total_fetched_file_size: timeseries(RATE, SUM),
    quicksand_fetched_file_size: timeseries(RATE, SUM),
}

mod ops {
    pub static CLIENTTELEMETRY: &str = "clienttelemetry";
    pub static HELLO: &str = "hello";
    pub static UNBUNDLE: &str = "unbundle";
    pub static HEADS: &str = "heads";
    pub static LOOKUP: &str = "lookup";
    pub static LISTKEYS: &str = "listkeys";
    pub static LISTKEYSPATTERNS: &str = "listkeyspatterns";
    pub static KNOWN: &str = "known";
    pub static KNOWNNODES: &str = "knownnodes";
    pub static BETWEEN: &str = "between";
    pub static GETBUNDLE: &str = "getbundle";
    pub static GETTREEPACK: &str = "gettreepack";
    pub static GETFILES: &str = "getfiles";
    pub static GETPACKV1: &str = "getpackv1";
    pub static GETPACKV2: &str = "getpackv2";
    pub static STREAMOUTSHALLOW: &str = "stream_out_shallow";
}

fn format_nodes_list(nodes: &Vec<HgChangesetId>) -> String {
    nodes.iter().map(|node| format!("{}", node)).join(" ")
}

fn format_manifests_set(nodes: &HashSet<HgManifestId>) -> String {
    nodes.iter().map(|node| format!("{}", node)).join(" ")
}

// Generic for HashSet, Vec, etc...
fn format_utf8_bytes_list<T, C>(entries: C) -> String
where
    T: AsRef<[u8]>,
    C: IntoIterator<Item = T>,
{
    entries
        .into_iter()
        .map(|entry| String::from_utf8_lossy(entry.as_ref()).into_owned())
        .join(",")
}

fn timeout_duration() -> Duration {
    Duration::from_secs(15 * 60)
}

fn clone_timeout_duration() -> Duration {
    // clone requests can be rather long. Let's bump the timeout
    Duration::from_secs(30 * 60)
}

fn getfiles_timeout_duration() -> Duration {
    // getfiles requests can be rather long. Let's bump the timeout
    Duration::from_secs(90 * 60)
}

fn process_timeout_error(err: TimeoutError<Error>) -> Error {
    match err.into_inner() {
        Some(err) => err,
        None => err_msg("timeout"),
    }
}

fn process_stream_timeout_error(err: StreamTimeoutError) -> Error {
    match err {
        StreamTimeoutError::Error(err) => err,
        StreamTimeoutError::Timeout => err_msg("timeout"),
    }
}

fn wireprotocaps() -> Vec<String> {
    vec![
        "clienttelemetry".to_string(),
        "lookup".to_string(),
        "known".to_string(),
        "getbundle".to_string(),
        "unbundle=HG10GZ,HG10BZ,HG10UN".to_string(),
        "gettreepack".to_string(),
        "remotefilelog".to_string(),
        "pushkey".to_string(),
        "stream-preferred".to_string(),
        "stream_option".to_string(),
        "streamreqs=generaldelta,lz4revlog,revlogv1".to_string(),
        "treeonly".to_string(),
        "knownnodes".to_string(),
    ]
}

fn bundle2caps(support_bundle2_listkeys: bool) -> String {
    let caps = {
        let mut caps = vec![
            ("HG20", vec![]),
            ("changegroup", vec!["02"]),
            ("b2x:infinitepush", vec![]),
            ("b2x:infinitepushscratchbookmarks", vec![]),
            ("pushkey", vec![]),
            ("treemanifestserver", vec!["True"]),
            ("b2x:rebase", vec![]),
            ("b2x:rebasepackpart", vec![]),
            ("phases", vec!["heads"]),
            ("obsmarkers", vec!["V1"]),
        ];

        if support_bundle2_listkeys {
            caps.push(("listkeys", vec![]))
        }
        caps
    };

    let mut encodedcaps = vec![];

    for &(ref key, ref value) in &caps {
        let encodedkey = key.to_string();
        if value.len() > 0 {
            let encodedvalue = value.join(",");
            encodedcaps.push([encodedkey, encodedvalue].join("="));
        } else {
            encodedcaps.push(encodedkey)
        }
    }

    percent_encode(&encodedcaps.join("\n"))
}

#[derive(Clone)]
pub struct RepoClient {
    repo: MononokeRepo,
    ctx: CoreContext,
    // Percent of returned entries (filelogs, manifests, changesets) which content
    // will be hash validated
    hash_validation_percentage: usize,
    lca_hint: Arc<LeastCommonAncestorsHint>,
    phases_hint: Arc<Phases>,
    // Whether to save raw bundle2 content into the blobstore
    preserve_raw_bundle2: bool,
    // Whether to allow non-pushrebase pushes
    pure_push_allowed: bool,
    hook_manager: Arc<HookManager>,
    // There is a race condition in bookmarks handling in Mercurial, which needs protocol-level
    // fixes. See `test-bookmark-race.t` for a reproducer; the issue is that between discovery
    // and bookmark handling (listkeys), we can get new commits and a bookmark change.
    // The client then gets a bookmark that points to a commit it does not yet have, and ignores it.
    // We currently fix it by caching bookmarks at the beginning of discovery.
    // TODO: T45411456 Fix this by teaching the client to expect extra commits to correspond to the bookmarks.
    cached_pull_default_bookmarks_maybe_stale: Arc<Mutex<Option<HashMap<Vec<u8>, Vec<u8>>>>>,
    support_bundle2_listkeys: bool,
}

// Logs wireproto requests both to scuba and scribe.
// Scuba logs are used for analysis of performance of both shadow and prod Mononoke tiers
// Scribe logs are used for replaying prod wireproto requests on shadow tier. So
// Scribe logging should be disabled on shadow tier.
struct WireprotoLogger {
    scuba_logger: ScubaSampleBuilder,
    scribe_client: Arc<ScribeClientImplementation>,
    // This scribe category main purpose is to tail the prod requests and replay them
    // on shadow tier.
    wireproto_scribe_category: Option<String>,
    wireproto_command: &'static str,
    args: Option<serde_json::Value>,
    reponame: String,
}

impl WireprotoLogger {
    fn new(
        scuba_logger: ScubaSampleBuilder,
        wireproto_command: &'static str,
        args: Option<serde_json::Value>,
        wireproto_scribe_category: Option<String>,
        reponame: String,
    ) -> Self {
        let mut logger = Self {
            scuba_logger,
            scribe_client: Arc::new(ScribeClientImplementation::new()),
            wireproto_scribe_category,
            wireproto_command,
            args: args.clone(),
            reponame,
        };
        logger.scuba_logger.add("command", logger.wireproto_command);

        if let Some(args) = args.clone() {
            if let Ok(args) = serde_json::to_string(&args) {
                logger.add_trimmed_scuba_field("command_args", args);
            }
        }
        logger.args = args;

        logger.scuba_logger.log_with_msg("Start processing", None);
        logger
    }

    fn set_args(&mut self, args: Option<serde_json::Value>) {
        self.args = args;
    }

    fn add_trimmed_scuba_field(&mut self, args_name: &str, args: String) {
        // Scuba does not support columns that are too long, we have to trim it
        let limit = ::std::cmp::min(args.len(), 1000);
        self.scuba_logger.add(args_name, &args[..limit]);
    }

    fn add_perf_counters_from_ctx(&mut self, key: &str, ctx: CoreContext) {
        if let Ok(counters) = serde_json::to_string(&ctx.perf_counters()) {
            self.add_trimmed_scuba_field(key, counters);
        }
    }

    fn finish_stream_wireproto_processing(&mut self, stats: &StreamStats, ctx: CoreContext) {
        self.scuba_logger
            .add_stream_stats(&stats)
            .log_with_msg("Command processed", None);

        if let Some(ref wireproto_scribe_category) = self.wireproto_scribe_category {
            let mut builder = ScubaSampleBuilder::with_discard();
            builder.add_common_server_data();
            match self.args {
                Some(ref args) => {
                    builder.add("args", args.to_string());
                }
                None => {
                    builder.add("args", "");
                }
            };
            builder.add("command", self.wireproto_command);
            builder.add("duration", stats.completion_time.as_millis_unchecked());
            builder.add("source_control_server_type", "mononoke");
            builder.add("mononoke_session_uuid", ctx.session().to_string());
            builder.add("reponame", self.reponame.as_str());

            // We can't really do anything with the errors, so let's ignore it
            let sample = builder.get_sample();
            if let Ok(sample_json) = sample.to_json() {
                let _ = self
                    .scribe_client
                    .offer(&wireproto_scribe_category, &sample_json.to_string());
            }
        }
    }
}

impl RepoClient {
    pub fn new(
        repo: MononokeRepo,
        ctx: CoreContext,
        hash_validation_percentage: usize,
        lca_hint: Arc<LeastCommonAncestorsHint>,
        phases_hint: Arc<Phases>,
        preserve_raw_bundle2: bool,
        pure_push_allowed: bool,
        hook_manager: Arc<HookManager>,
        support_bundle2_listkeys: bool,
    ) -> Self {
        RepoClient {
            repo,
            ctx,
            hash_validation_percentage,
            lca_hint,
            phases_hint,
            preserve_raw_bundle2,
            pure_push_allowed,
            hook_manager,
            cached_pull_default_bookmarks_maybe_stale: Arc::new(Mutex::new(None)),
            support_bundle2_listkeys,
        }
    }

    fn prepared_ctx(&self, op: &str, args: Option<String>) -> CoreContext {
        self.ctx.with_scuba_initialization(|mut scuba_logger| {
            scuba_logger.add("command", op);

            if let Some(args) = args {
                scuba_logger.add("command_args", args);
            }

            scuba_logger.log_with_msg("Start processing", None);
            scuba_logger
        })
    }

    fn get_pull_default_bookmarks_maybe_stale(
        &self,
    ) -> impl Future<Item = HashMap<Vec<u8>, Vec<u8>>, Error = Error> {
        let maybe_cache = self
            .cached_pull_default_bookmarks_maybe_stale
            .lock()
            .expect("lock poisoned");

        match *maybe_cache {
            None => self
                .repo
                .blobrepo()
                .get_pull_default_bookmarks_maybe_stale(self.ctx.clone())
                .map(|(book, cs): (Bookmark, HgChangesetId)| {
                    let hash: Vec<u8> = cs.into_nodehash().to_hex().into();
                    (book.into_name(), hash)
                })
                .collect()
                .map({
                    cloned!(
                        self.cached_pull_default_bookmarks_maybe_stale,
                        self.support_bundle2_listkeys
                    );
                    move |bookmarks| {
                        let bookiter = bookmarks
                            .into_iter()
                            .map(|(name, value)| (Vec::from(name.to_string()), value));
                        if support_bundle2_listkeys {
                            let mut maybe_cache = cached_pull_default_bookmarks_maybe_stale
                                .lock()
                                .expect("lock poisoned");
                            maybe_cache
                                .get_or_insert_with(|| HashMap::from_iter(bookiter))
                                .clone()
                        } else {
                            HashMap::from_iter(bookiter)
                        }
                    }
                })
                .timeout(timeout_duration())
                .map_err(process_timeout_error)
                .left_future(),
            Some(ref bookmarks) => future::ok(bookmarks.clone()).right_future(),
        }
    }

    fn create_bundle(&self, args: GetbundleArgs) -> Result<BoxStream<Bytes, Error>> {
        let blobrepo = self.repo.blobrepo();
        let mut bundle2_parts = vec![];

        let mut use_phases = args.phases;
        if use_phases {
            for cap in args.bundlecaps {
                if let Some((cap_name, caps)) = parse_utf8_getbundle_caps(&cap) {
                    if cap_name != "bundle2" {
                        continue;
                    }
                    if let Some(phases) = caps.get("phases") {
                        use_phases = phases.contains("heads");
                        break;
                    }
                }
            }
        }

        bundle2_parts.append(&mut bundle2_resolver::create_getbundle_response(
            self.ctx.clone(),
            blobrepo.clone(),
            args.common,
            args.heads,
            self.lca_hint.clone(),
            if use_phases {
                Some(self.phases_hint.clone())
            } else {
                None
            },
        )?);

        // listkeys bookmarks part is added separately.

        // TODO: generalize this to other listkey types
        // (note: just calling &b"bookmarks"[..] doesn't work because https://fburl.com/0p0sq6kp)
        if args.listkeys.contains(&b"bookmarks".to_vec()) {
            let items = self
                .get_pull_default_bookmarks_maybe_stale()
                .map(|bookmarks| stream::iter_ok(bookmarks))
                .flatten_stream();
            bundle2_parts.push(parts::listkey_part("bookmarks", items)?);
        }
        // TODO(stash): handle includepattern= and excludepattern=

        let compression = None;
        Ok(create_bundle_stream(bundle2_parts, compression).boxify())
    }

    fn gettreepack_untimed(&self, params: GettreepackArgs) -> BoxStream<Bytes, Error> {
        debug!(self.ctx.logger(), "gettreepack");

        let validate_hash = rand::random::<usize>() % 100 < self.hash_validation_percentage;
        let changed_entries = gettreepack_entries(self.ctx.clone(), self.repo.blobrepo(), params)
            .filter({
                let mut used_hashes = HashSet::new();
                move |entry| used_hashes.insert(entry.0.get_hash())
            })
            .map({
                cloned!(self.ctx);
                let blobrepo = self.repo.blobrepo().clone();
                move |(entry, basepath)| {
                    ctx.perf_counters()
                        .increment_counter("gettreepack_num_treepacks");
                    fetch_treepack_part_input(
                        ctx.clone(),
                        &blobrepo,
                        entry,
                        basepath,
                        validate_hash,
                    )
                }
            });

        let part = parts::treepack_part(changed_entries);
        // Mercurial currently hangs while trying to read compressed bundles over the wire:
        // https://bz.mercurial-scm.org/show_bug.cgi?id=5646
        // TODO: possibly enable compression support once this is fixed.
        let compression = None;
        part.into_future()
            .map(move |part| create_bundle_stream(vec![part], compression))
            .flatten_stream()
            .boxify()
    }

    fn getpack(
        &self,
        params: BoxStream<(MPath, Vec<HgFileNodeId>), Error>,
        version: u8,
        mut wireproto_logger: WireprotoLogger,
    ) -> BoxStream<Bytes, Error> {
        // We buffer all parameters in memory so that we can log them.
        // That shouldn't be a problem because requests are quite small
        let getpack_params = Arc::new(Mutex::new(vec![]));
        let ctx = self.ctx.clone();
        let repo = self.repo.blobrepo().clone();

        let validate_hash =
            rand::thread_rng().gen_ratio(self.hash_validation_percentage as u32, 100);
        let getpack_buffer_size = 500;
        // Let's fetch the whole request before responding.
        // That's prevents deadlocks, because hg client doesn't start reading the response
        // before all the arguments were sent.
        let s = params
            .collect()
            .map(|v| stream::iter_ok(v.into_iter()))
            .flatten_stream()
            .map({
                cloned!(ctx, getpack_params, repo);
                move |(path, filenodes)| {
                    {
                        let mut getpack_params = getpack_params.lock().unwrap();
                        getpack_params.push((path.clone(), filenodes.clone()));
                    }
                    let mut sizes_futs = vec![];
                    for filenode in filenodes.iter() {
                        let f = repo.get_file_size(ctx.clone(), *filenode);
                        sizes_futs.push(f);
                    }

                    let contents: Vec<_> = filenodes
                        .iter()
                        .map(|filenode| {
                            repo.get_raw_hg_content(ctx.clone(), *filenode, validate_hash)
                                .map({
                                    cloned!(filenode);
                                    move |content| (filenode, content.into_inner())
                                })
                        })
                        .collect();

                    let history_fut = get_unordered_file_history_for_multiple_nodes(
                        ctx.clone(),
                        repo.clone(),
                        filenodes.into_iter().collect(),
                        &path,
                    )
                    .collect();

                    let contents_fut = future::join_all(contents.into_iter())
                        .join(history_fut)
                        .map(move |(contents, history)| (path, contents, history))
                        .boxify();

                    future::join_all(sizes_futs.into_iter())
                        .map(move |filenode_sizes| (contents_fut, filenode_sizes.into_iter().sum()))
                }
            })
            .buffered(getpack_buffer_size);

        let params = BufferedParams {
            weight_limit: 100_000_000,
            buffer_size: getpack_buffer_size,
        };
        let s = s
            .buffered_weight_limited(params)
            .whole_stream_timeout(getfiles_timeout_duration())
            .map_err(process_stream_timeout_error)
            .map({
                cloned!(ctx);
                move |(path, contents, history)| {
                    let mut res = vec![wirepack::Part::HistoryMeta {
                        path: RepoPath::FilePath(path.clone()),
                        entry_count: history.len() as u32,
                    }];

                    let history = history.into_iter().map(|history_entry| {
                        let (p1, p2, copy_from) = convert_parents_to_remotefilelog_format(
                            history_entry.parents(),
                            history_entry.copyfrom().as_ref(),
                        );

                        wirepack::Part::History(wirepack::HistoryEntry {
                            node: history_entry.filenode().into_nodehash(),
                            p1: p1.into_nodehash(),
                            p2: p2.into_nodehash(),
                            linknode: history_entry.linknode().into_nodehash(),
                            copy_from: copy_from.cloned().map(RepoPath::FilePath),
                        })
                    });
                    res.extend(history);

                    res.push(wirepack::Part::DataMeta {
                        path: RepoPath::FilePath(path),
                        entry_count: contents.len() as u32,
                    });
                    for (filenode, content) in contents {
                        let content = content.to_vec();
                        ctx.perf_counters()
                            .set_max_counter("getpack_max_file_size", content.len() as i64);
                        res.push(wirepack::Part::Data(wirepack::DataEntry {
                            node: filenode.into_nodehash(),
                            delta_base: NULL_HASH,
                            delta: Delta::new_fulltext(content),
                            version,
                        }));
                    }
                    stream::iter_ok(res.into_iter())
                }
            })
            .flatten()
            .chain(stream::once(Ok(wirepack::Part::End)));

        wirepack::packer::WirePackPacker::new(s, wirepack::Kind::File)
            .and_then(|chunk| chunk.into_bytes())
            .inspect({
                cloned!(self.ctx);
                move |bytes| {
                    let len = bytes.len() as i64;
                    ctx.perf_counters()
                        .add_to_counter("getpack_response_size", len);

                    STATS::total_fetched_file_size.add_value(len as i64);
                    if ctx.is_quicksand() {
                        STATS::quicksand_fetched_file_size.add_value(len as i64);
                    }
                }
            })
            .timed({
                cloned!(self.ctx);
                move |stats, _| {
                    let encoded_params = {
                        let getpack_params = getpack_params.lock().unwrap();
                        let mut encoded_params = vec![];
                        for (path, filenodes) in getpack_params.iter() {
                            let mut encoded_filenodes = vec![];
                            for filenode in filenodes {
                                encoded_filenodes.push(format!("{}", filenode));
                            }
                            encoded_params.push((
                                String::from_utf8_lossy(&path.to_vec()).to_string(),
                                encoded_filenodes,
                            ));
                        }
                        encoded_params
                    };

                    ctx.perf_counters()
                        .add_to_counter("getpack_num_files", encoded_params.len() as i64);

                    wireproto_logger.set_args(Some(json! {encoded_params}));
                    wireproto_logger.add_perf_counters_from_ctx("extra_context", ctx.clone());
                    wireproto_logger.finish_stream_wireproto_processing(&stats, ctx);
                    Ok(())
                }
            })
            .boxify()
    }

    fn wireproto_logger(
        &self,
        wireproto_command: &'static str,
        args: Option<serde_json::Value>,
    ) -> WireprotoLogger {
        WireprotoLogger::new(
            self.ctx.scuba().clone(),
            wireproto_command,
            args,
            self.ctx.wireproto_scribe_category().clone(),
            self.repo.reponame().clone(),
        )
    }
}

impl HgCommands for RepoClient {
    // @wireprotocommand('between', 'pairs')
    fn between(
        &self,
        pairs: Vec<(HgChangesetId, HgChangesetId)>,
    ) -> HgCommandRes<Vec<Vec<HgChangesetId>>> {
        info!(self.ctx.logger(), "between pairs {:?}", pairs);

        struct ParentStream<CS> {
            ctx: CoreContext,
            repo: MononokeRepo,
            n: HgChangesetId,
            bottom: HgChangesetId,
            wait_cs: Option<CS>,
        };

        impl<CS> ParentStream<CS> {
            fn new(
                ctx: CoreContext,
                repo: &MononokeRepo,
                top: HgChangesetId,
                bottom: HgChangesetId,
            ) -> Self {
                ParentStream {
                    ctx,
                    repo: repo.clone(),
                    n: top,
                    bottom,
                    wait_cs: None,
                }
            }
        }

        impl Stream for ParentStream<BoxFuture<HgBlobChangeset, hgproto::Error>> {
            type Item = HgChangesetId;
            type Error = hgproto::Error;

            fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
                if self.n == self.bottom || self.n.into_nodehash() == NULL_HASH {
                    return Ok(Async::Ready(None));
                }

                self.wait_cs = self.wait_cs.take().or_else(|| {
                    Some(
                        self.repo
                            .blobrepo()
                            .get_changeset_by_changesetid(self.ctx.clone(), self.n),
                    )
                });
                let cs = try_ready!(self.wait_cs.as_mut().unwrap().poll());
                self.wait_cs = None; // got it

                let p = cs.p1().unwrap_or(NULL_HASH);
                let prev_n = mem::replace(&mut self.n, HgChangesetId::new(p));

                Ok(Async::Ready(Some(prev_n)))
            }
        }

        let mut scuba_logger = self.prepared_ctx(ops::BETWEEN, None).scuba().clone();

        // TODO(jsgf): do pairs in parallel?
        // TODO: directly return stream of streams
        cloned!(self.ctx, self.repo);
        stream::iter_ok(pairs.into_iter())
            .and_then(move |(top, bottom)| {
                let mut f = 1;
                ParentStream::new(ctx.clone(), &repo, top, bottom)
                    .enumerate()
                    .filter(move |&(i, _)| {
                        if i == f {
                            f *= 2;
                            true
                        } else {
                            false
                        }
                    })
                    .map(|(_, v)| v)
                    .collect()
            })
            .collect()
            .timeout(timeout_duration())
            .map_err(process_timeout_error)
            .traced(self.ctx.trace(), ops::BETWEEN, trace_args!())
            .timed(move |stats, _| {
                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Command processed", None);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('clienttelemetry')
    fn clienttelemetry(&self, _args: HashMap<Vec<u8>, Vec<u8>>) -> HgCommandRes<String> {
        info!(self.ctx.logger(), "clienttelemetry");

        let fallback_hostname = "<no hostname found>";
        let hostname = match FbWhoAmI::new() {
            Ok(fbwhoami) => fbwhoami.get_name().unwrap_or(fallback_hostname).to_string(),
            Err(_) => fallback_hostname.to_string(),
        };

        let mut scuba_logger = self
            .prepared_ctx(ops::CLIENTTELEMETRY, None)
            .scuba()
            .clone();

        future::ok(hostname)
            .timeout(timeout_duration())
            .map_err(process_timeout_error)
            .traced(self.ctx.trace(), ops::CLIENTTELEMETRY, trace_args!())
            .timed(move |stats, _| {
                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Command processed", None);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('heads')
    fn heads(&self) -> HgCommandRes<HashSet<HgChangesetId>> {
        // Get a stream of heads and collect them into a HashSet
        // TODO: directly return stream of heads
        info!(self.ctx.logger(), "heads");
        let mut scuba_logger = self.prepared_ctx(ops::HEADS, None).scuba().clone();

        // We get all bookmarks while handling heads to fix the race demonstrated in
        // test-bookmark-race.t - this fixes bookmarks at the moment the client starts discovery
        // NB: Getting bookmarks is only done here to ensure that they are cached at the beginning
        // of discovery - this function is meant to get heads only.
        self.get_pull_default_bookmarks_maybe_stale()
            .join(
                self.repo
                    .blobrepo()
                    .get_heads_maybe_stale(self.ctx.clone())
                    .collect()
                    .map(|v| v.into_iter().collect())
                    .from_err(),
            )
            .map(|(_, r)| r)
            .timeout(timeout_duration())
            .map_err(process_timeout_error)
            .traced(self.ctx.trace(), ops::HEADS, trace_args!())
            .timed(move |stats, _| {
                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Command processed", None);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('lookup', 'key')
    fn lookup(&self, key: String) -> HgCommandRes<Bytes> {
        info!(self.ctx.logger(), "lookup: {:?}", key);
        // TODO(stash): T25928839 lookup should support prefixes
        let repo = self.repo.blobrepo().clone();
        let mut scuba_logger = self.prepared_ctx(ops::LOOKUP, None).scuba().clone();

        fn generate_resp_buf(success: bool, message: &[u8]) -> Bytes {
            let mut buf = BytesMut::with_capacity(message.len() + 3);
            if success {
                buf.put(b'1');
            } else {
                buf.put(b'0');
            }
            buf.put(b' ');
            buf.put(message);
            buf.put(b'\n');
            buf.freeze()
        }

        fn check_bookmark_exists(
            ctx: CoreContext,
            repo: BlobRepo,
            bookmark: BookmarkName,
        ) -> HgCommandRes<Bytes> {
            repo.get_bookmark(ctx, &bookmark)
                .map(move |csid| match csid {
                    Some(csid) => generate_resp_buf(true, csid.to_hex().as_bytes()),
                    None => generate_resp_buf(false, format!("{} not found", bookmark).as_bytes()),
                })
                .boxify()
        }

        let node = HgChangesetId::from_str(&key).ok();
        let bookmark = BookmarkName::new(&key).ok();

        let lookup_fut = match (node, bookmark) {
            (Some(node), Some(bookmark)) => {
                let csid = node;
                repo.changeset_exists(self.ctx.clone(), csid)
                    .and_then({
                        cloned!(self.ctx);
                        move |exists| {
                            if exists {
                                Ok(generate_resp_buf(true, node.to_hex().as_bytes()))
                                    .into_future()
                                    .boxify()
                            } else {
                                check_bookmark_exists(ctx, repo, bookmark)
                            }
                        }
                    })
                    .boxify()
            }
            (None, Some(bookmark)) => check_bookmark_exists(self.ctx.clone(), repo, bookmark),
            // Failed to parse as a hash or bookmark.
            _ => Ok(generate_resp_buf(false, "invalid input".as_bytes()))
                .into_future()
                .boxify(),
        };

        lookup_fut
            .timeout(timeout_duration())
            .map_err(process_timeout_error)
            .traced(self.ctx.trace(), ops::LOOKUP, trace_args!())
            .timed(move |stats, _| {
                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Command processed", None);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('known', 'nodes *'), but the '*' is ignored
    fn known(&self, nodes: Vec<HgChangesetId>) -> HgCommandRes<Vec<bool>> {
        if nodes.len() > MAX_NODES_TO_LOG {
            info!(
                self.ctx.logger(),
                "known: {:?}...",
                &nodes[..MAX_NODES_TO_LOG]
            );
        } else {
            info!(self.ctx.logger(), "known: {:?}", nodes);
        }
        let blobrepo = self.repo.blobrepo().clone();

        let mut scuba_logger = self.prepared_ctx(ops::KNOWN, None).scuba().clone();

        let nodes_len = nodes.len();

        let phases_hint = self.phases_hint.clone();

        cloned!(self.ctx);
        blobrepo
            .get_hg_bonsai_mapping(ctx.clone(), nodes.clone())
            .map(|hg_bcs_mapping| {
                let mut bcs_ids = vec![];
                let mut bcs_hg_mapping = hashmap! {};

                for (hg, bcs) in hg_bcs_mapping {
                    bcs_ids.push(bcs);
                    bcs_hg_mapping.insert(bcs, hg);
                }
                (bcs_ids, bcs_hg_mapping)
            })
            .and_then(move |(bcs_ids, bcs_hg_mapping)| {
                phases_hint
                    .get_public(ctx, blobrepo, bcs_ids)
                    .map(move |public_csids| {
                        public_csids
                            .into_iter()
                            .filter_map(|csid| bcs_hg_mapping.get(&csid).cloned())
                            .collect::<HashSet<_>>()
                    })
            })
            .map(move |found_hg_changesets| {
                nodes
                    .into_iter()
                    .map(move |node| found_hg_changesets.contains(&node))
                    .collect::<Vec<_>>()
            })
            .timeout(timeout_duration())
            .map_err(process_timeout_error)
            .traced(self.ctx.trace(), ops::KNOWN, trace_args!())
            .timed(move |stats, known_nodes| {
                if let Ok(known) = known_nodes {
                    let extra_context = json!({
                        "num_known": known.len(),
                        "num_unknown": nodes_len - known.len(),
                    })
                    .to_string();

                    scuba_logger.add("extra_context", extra_context);
                }

                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Command processed", None);
                Ok(())
            })
            .boxify()
    }

    fn knownnodes(&self, nodes: Vec<HgChangesetId>) -> HgCommandRes<Vec<bool>> {
        let blobrepo = self.repo.blobrepo().clone();

        let mut scuba_logger = self.prepared_ctx(ops::KNOWNNODES, None).scuba().clone();

        let nodes_len = nodes.len();

        blobrepo
            .get_hg_bonsai_mapping(self.ctx.clone(), nodes.clone())
            .map(|hg_bcs_mapping| {
                let hg_bcs_mapping: HashMap<_, _> = hg_bcs_mapping.into_iter().collect();
                nodes
                    .into_iter()
                    .map(move |node| hg_bcs_mapping.contains_key(&node))
                    .collect::<Vec<_>>()
            })
            .timeout(timeout_duration())
            .map_err(process_timeout_error)
            .traced(self.ctx.trace(), ops::KNOWNNODES, trace_args!())
            .timed(move |stats, known_nodes| {
                if let Ok(known) = known_nodes {
                    let extra_context = json!({
                        "num_known": known.len(),
                        "num_unknown": nodes_len - known.len(),
                    })
                    .to_string();

                    scuba_logger.add("extra_context", extra_context);
                }

                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Command processed", None);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('getbundle', '*')
    fn getbundle(&self, args: GetbundleArgs) -> BoxStream<Bytes, Error> {
        info!(self.ctx.logger(), "Getbundle: {:?}", args);

        let value = json!({
            "bundlecaps": format_utf8_bytes_list(&args.bundlecaps),
            "common": format_nodes_list(&args.common),
            "heads": format_nodes_list(&args.heads),
            "listkeys": format_utf8_bytes_list(&args.listkeys),
        });
        let value = json!(vec![value]);
        let mut wireproto_logger = self.wireproto_logger(ops::GETBUNDLE, Some(value));
        cloned!(self.ctx);

        match self.create_bundle(args) {
            Ok(res) => res.boxify(),
            Err(err) => stream::once(Err(err)).boxify(),
        }
        .whole_stream_timeout(timeout_duration())
        .map_err(process_stream_timeout_error)
        .traced(self.ctx.trace(), ops::GETBUNDLE, trace_args!())
        .timed(move |stats, _| {
            STATS::getbundle_ms.add_value(stats.completion_time.as_millis_unchecked() as i64);
            wireproto_logger.add_perf_counters_from_ctx("extra_context", ctx.clone());
            wireproto_logger.finish_stream_wireproto_processing(&stats, ctx);
            Ok(())
        })
        .boxify()
    }

    // @wireprotocommand('hello')
    fn hello(&self) -> HgCommandRes<HashMap<String, Vec<String>>> {
        info!(self.ctx.logger(), "Hello -> capabilities");

        let mut res = HashMap::new();
        let mut caps = wireprotocaps();
        caps.push(format!(
            "bundle2={}",
            bundle2caps(self.support_bundle2_listkeys)
        ));
        res.insert("capabilities".to_string(), caps);

        let mut scuba_logger = self.prepared_ctx(ops::HELLO, None).scuba().clone();

        future::ok(res)
            .timeout(timeout_duration())
            .map_err(process_timeout_error)
            .traced(self.ctx.trace(), ops::HELLO, trace_args!())
            .timed(move |stats, _| {
                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Command processed", None);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('listkeys', 'namespace')
    fn listkeys(&self, namespace: String) -> HgCommandRes<HashMap<Vec<u8>, Vec<u8>>> {
        info!(self.ctx.logger(), "listkeys: {}", namespace);
        if namespace == "bookmarks" {
            let mut scuba_logger = self.prepared_ctx(ops::LISTKEYS, None).scuba().clone();

            self.get_pull_default_bookmarks_maybe_stale()
                .traced(self.ctx.trace(), ops::LISTKEYS, trace_args!())
                .timed(move |stats, _| {
                    scuba_logger
                        .add_future_stats(&stats)
                        .log_with_msg("Command processed", None);
                    Ok(())
                })
                .boxify()
        } else {
            info!(
                self.ctx.logger(),
                "unsupported listkeys namespace: {}", namespace
            );
            future::ok(HashMap::new()).boxify()
        }
    }

    // @wireprotocommand('listkeyspatterns', 'namespace', 'patterns *')
    fn listkeyspatterns(
        &self,
        namespace: String,
        patterns: Vec<String>,
    ) -> HgCommandRes<BTreeMap<String, HgChangesetId>> {
        info!(
            self.ctx.logger(),
            "listkeyspatterns: {} {:?}", namespace, patterns
        );
        if namespace != "bookmarks" {
            info!(
                self.ctx.logger(),
                "unsupported listkeyspatterns namespace: {}", namespace,
            );
            return future::err(format_err!(
                "unsupported listkeyspatterns namespace: {}",
                namespace
            ))
            .boxify();
        }

        let mut scuba_logger = self
            .prepared_ctx(ops::LISTKEYSPATTERNS, None)
            .scuba()
            .clone();

        let queries = patterns.into_iter().map({
            let max = self.repo.list_keys_patterns_max();
            let repo = self.repo.blobrepo();
            cloned!(self.ctx);
            move |pattern| {
                if pattern.ends_with("*") {
                    // prefix match
                    let prefix = try_boxfuture!(BookmarkPrefix::new(&pattern[..pattern.len() - 1]));
                    repo.get_bookmarks_by_prefix_maybe_stale(ctx.clone(), &prefix, max)
                        .map(|(bookmark, cs_id): (Bookmark, HgChangesetId)| {
                            (bookmark.into_name().to_string(), cs_id)
                        })
                        .collect()
                        .boxify()
                } else {
                    // literal match
                    let bookmark = try_boxfuture!(BookmarkName::new(&pattern));
                    repo.get_bookmark(ctx.clone(), &bookmark)
                        .map(move |cs_id| match cs_id {
                            Some(cs_id) => vec![(pattern, cs_id)],
                            None => Vec::new(),
                        })
                        .boxify()
                }
            }
        });

        stream::futures_unordered(queries)
            .concat2()
            .map(|bookmarks| bookmarks.into_iter().collect())
            .timeout(timeout_duration())
            .map_err(process_timeout_error)
            .traced(self.ctx.trace(), ops::LISTKEYS, trace_args!())
            .timed(move |stats, _| {
                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Command processed", None);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('unbundle')
    fn unbundle(
        &self,
        heads: Vec<String>,
        stream: BoxStream<Bundle2Item, Error>,
        maybe_full_content: Option<Arc<Mutex<Bytes>>>,
    ) -> HgCommandRes<Bytes> {
        let client = self.clone();
        let pure_push_allowed = self.pure_push_allowed;
        cloned!(self.hook_manager);

        // Kill the saved set of bookmarks here - the unbundle may change them, and the next
        // command in sequence will need to fetch a new set
        let _ = self
            .cached_pull_default_bookmarks_maybe_stale
            .lock()
            .expect("lock poisoned")
            .take();

        self.repo
            .readonly()
            // Assume read only if we have an error.
            .or_else(|_| ok(RepoReadOnly::ReadOnly("Failed to fetch repo lock status".to_string())))
            .and_then(move |read_write| {
                let ctx = client.prepared_ctx(ops::UNBUNDLE, None);
                let mut scuba_logger = ctx.scuba().clone();

                let res = bundle2_resolver::resolve(
                    ctx.with_logger_kv(o!("command" => "unbundle")),
                    client.repo.blobrepo().clone(),
                    client.repo.pushrebase_params().clone(),
                    client.repo.bookmark_attrs(),
                    client.repo.infinitepush().clone(),
                    heads,
                    stream,
                    hook_manager,
                    client.lca_hint.clone(),
                    client.phases_hint.clone(),
                    read_write,
                    maybe_full_content,
                    pure_push_allowed,
                );

                res.timeout(timeout_duration())
                    .map_err(process_timeout_error)
                    .traced(client.ctx.trace(), ops::UNBUNDLE, trace_args!())
                    .timed(move |stats, _| {
                        if let Ok(counters) = serde_json::to_string(&ctx.perf_counters()) {
                            scuba_logger.add("extra_context", counters);
                        }
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("Command processed", None);
                        Ok(())
                    })
            })
            .boxify()
    }

    // @wireprotocommand('gettreepack', 'rootdir mfnodes basemfnodes directories')
    fn gettreepack(&self, params: GettreepackArgs) -> BoxStream<Bytes, Error> {
        let args = json!({
            "rootdir": String::from_utf8_lossy(&params.rootdir),
            "mfnodes": format_manifests_set(&params.mfnodes),
            "basemfnodes": format_manifests_set(&params.basemfnodes),
            "directories": format_utf8_bytes_list(&params.directories),
        });
        let args = json!(vec![args]);
        let mut wireproto_logger = self.wireproto_logger(ops::GETTREEPACK, Some(args));

        self.gettreepack_untimed(params)
            .whole_stream_timeout(timeout_duration())
            .map_err(process_stream_timeout_error)
            .traced(self.ctx.trace(), ops::GETTREEPACK, trace_args!())
            .inspect({
                cloned!(self.ctx);
                move |bytes| {
                    ctx.perf_counters()
                        .add_to_counter("gettreepack_response_size", bytes.len() as i64);
                    STATS::total_tree_size.add_value(bytes.len() as i64);
                    STATS::total_tree_count.add_value(1);
                    if ctx.is_quicksand() {
                        STATS::quicksand_tree_size.add_value(bytes.len() as i64);
                        STATS::quicksand_tree_count.add_value(1);
                    }
                }
            })
            .timed({
                cloned!(self.ctx);
                move |stats, _| {
                    STATS::gettreepack_ms
                        .add_value(stats.completion_time.as_millis_unchecked() as i64);
                    wireproto_logger.add_perf_counters_from_ctx("extra_context", ctx.clone());
                    wireproto_logger.finish_stream_wireproto_processing(&stats, ctx);
                    Ok(())
                }
            })
            .boxify()
    }

    // @wireprotocommand('getfiles', 'files*')
    fn getfiles(&self, params: BoxStream<(HgFileNodeId, MPath), Error>) -> BoxStream<Bytes, Error> {
        info!(self.ctx.logger(), "getfiles");

        let mut wireproto_logger = self.wireproto_logger(ops::GETFILES, None);
        let this = self.clone();
        // TODO(stash): make it configurable
        let getfiles_buffer_size = 100;
        // We buffer all parameters in memory so that we can log them.
        // That shouldn't be a problem because requests are quite small
        let getfiles_params = Arc::new(Mutex::new(vec![]));

        let validate_hash = rand::random::<usize>() % 100 < self.hash_validation_percentage;
        params
            .map({
                cloned!(getfiles_params);
                move |param| {
                    let mut getfiles_params = getfiles_params.lock().unwrap();
                    getfiles_params.push(param.clone());
                    param
                }
            })
            .map({
                cloned!(self.ctx);
                move |(node, path)| {
                    let repo = this.repo.clone();
                    create_remotefilelog_blob(
                        ctx.clone(),
                        repo.blobrepo().clone(),
                        node,
                        path.clone(),
                        repo.lfs_params().clone(),
                        validate_hash,
                    )
                    .traced(
                        this.ctx.trace(),
                        ops::GETFILES,
                        trace_args!("node" => node.to_string(), "path" =>  path.to_string()),
                    )
                    .timed({
                        cloned!(ctx);
                        move |stats, _| {
                            STATS::getfiles_ms
                                .add_value(stats.completion_time.as_millis_unchecked() as i64);
                            let completion_time =
                                stats.completion_time.as_millis_unchecked() as i64;
                            ctx.perf_counters()
                                .set_max_counter("getfiles_max_latency", completion_time);
                            Ok(())
                        }
                    })
                }
            })
            .buffered(getfiles_buffer_size)
            .inspect({
                cloned!(self.ctx);
                move |bytes| {
                    let len = bytes.len() as i64;
                    ctx.perf_counters()
                        .add_to_counter("getfiles_response_size", len);
                    ctx.perf_counters()
                        .set_max_counter("getfiles_max_file_size", len);

                    STATS::total_fetched_file_size.add_value(len as i64);
                    if ctx.is_quicksand() {
                        STATS::quicksand_fetched_file_size.add_value(len as i64);
                    }
                }
            })
            .whole_stream_timeout(getfiles_timeout_duration())
            .map_err(process_stream_timeout_error)
            .timed({
                cloned!(self.ctx);
                move |stats, _| {
                    let encoded_params = {
                        let getfiles_params = getfiles_params.lock().unwrap();
                        let mut encoded_params = vec![];
                        for (node, path) in getfiles_params.iter() {
                            encoded_params.push(vec![
                                format!("{}", node),
                                String::from_utf8_lossy(&path.to_vec()).to_string(),
                            ]);
                        }
                        encoded_params
                    };

                    ctx.perf_counters()
                        .add_to_counter("getfiles_num_files", stats.count as i64);

                    wireproto_logger.set_args(Some(json! {encoded_params}));
                    wireproto_logger.add_perf_counters_from_ctx("extra_context", ctx.clone());
                    wireproto_logger.finish_stream_wireproto_processing(&stats, ctx);
                    Ok(())
                }
            })
            .boxify()
    }

    // @wireprotocommand('stream_out_shallow')
    fn stream_out_shallow(&self) -> BoxStream<Bytes, Error> {
        info!(self.ctx.logger(), "{}", ops::STREAMOUTSHALLOW);
        let mut wireproto_logger = self.wireproto_logger(ops::STREAMOUTSHALLOW, None);
        let changelog = match self.repo.streaming_clone() {
            None => Ok(RevlogStreamingChunks::new()).into_future().left_future(),
            Some(SqlStreamingCloneConfig {
                blobstore,
                fetcher,
                repoid,
            }) => fetcher
                .fetch_changelog(self.ctx.clone(), *repoid, blobstore.clone())
                .right_future(),
        };

        changelog
            .map({
                let ctx = self.ctx.clone();
                move |chunk| {
                    let data_blobs = chunk
                        .data_blobs
                        .into_iter()
                        .map(|fut| {
                            fut.timed({
                                let ctx = ctx.clone();
                                move |stats, _| {
                                    ctx.perf_counters().add_to_counter(
                                        "sum_manifold_poll_time",
                                        stats.poll_time.as_nanos_unchecked() as i64,
                                    );
                                    Ok(())
                                }
                            })
                        })
                        .collect();

                    let index_blobs = chunk
                        .index_blobs
                        .into_iter()
                        .map(|fut| {
                            fut.timed({
                                let ctx = ctx.clone();
                                move |stats, _| {
                                    ctx.perf_counters().add_to_counter(
                                        "sum_manifold_poll_time",
                                        stats.poll_time.as_nanos_unchecked() as i64,
                                    );
                                    Ok(())
                                }
                            })
                        })
                        .collect();

                    RevlogStreamingChunks {
                        data_size: chunk.data_size,
                        index_size: chunk.index_size,
                        data_blobs,
                        index_blobs,
                    }
                }
            })
            .map({
                let ctx = self.ctx.clone();
                move |changelog_chunks| {
                    debug!(
                        ctx.logger(),
                        "streaming changelog {} index bytes, {} data bytes",
                        changelog_chunks.index_size,
                        changelog_chunks.data_size
                    );
                    let mut response_header = Vec::new();
                    // TODO(t34058163): actually send a real streaming response, not an empty one
                    // Send OK response.
                    response_header.push(Bytes::from_static(b"0\n"));
                    // send header.
                    let total_size = changelog_chunks.index_size + changelog_chunks.data_size;
                    let file_count = 2;
                    let header = format!("{} {}\n", file_count, total_size);
                    response_header.push(header.into_bytes().into());
                    let response = stream::iter_ok(response_header);

                    fn build_file_stream(
                        name: &str,
                        size: usize,
                        data: Vec<BoxFuture<Bytes, Error>>,
                    ) -> impl Stream<Item = Bytes, Error = Error> + Send {
                        let header = format!("{}\0{}\n", name, size);

                        stream::once(Ok(header.into_bytes().into()))
                            .chain(stream::iter_ok(data.into_iter()).buffered(100))
                    }

                    response
                        .chain(build_file_stream(
                            "00changelog.i",
                            changelog_chunks.index_size,
                            changelog_chunks.index_blobs,
                        ))
                        .chain(build_file_stream(
                            "00changelog.d",
                            changelog_chunks.data_size,
                            changelog_chunks.data_blobs,
                        ))
                }
            })
            .flatten_stream()
            .whole_stream_timeout(clone_timeout_duration())
            .map_err(process_stream_timeout_error)
            .timed({
                let ctx = self.ctx.clone();
                move |stats, _| {
                    wireproto_logger.add_perf_counters_from_ctx("extra_context", ctx.clone());
                    wireproto_logger.finish_stream_wireproto_processing(&stats, ctx);
                    Ok(())
                }
            })
            .boxify()
    }

    // @wireprotocommand('getpackv1')
    fn getpackv1(
        &self,
        params: BoxStream<(MPath, Vec<HgFileNodeId>), Error>,
    ) -> BoxStream<Bytes, Error> {
        info!(self.ctx.logger(), "{}", ops::GETPACKV1);
        let wireproto_logger = self.wireproto_logger(ops::GETPACKV1, None);

        self.getpack(params, 1, wireproto_logger)
    }

    // @wireprotocommand('getpackv2')
    fn getpackv2(
        &self,
        params: BoxStream<(MPath, Vec<HgFileNodeId>), Error>,
    ) -> BoxStream<Bytes, Error> {
        info!(self.ctx.logger(), "{}", ops::GETPACKV2);
        let wireproto_logger = self.wireproto_logger(ops::GETPACKV2, None);

        self.getpack(params, 2, wireproto_logger)
    }

    // whether raw bundle2 contents should be preverved in the blobstore
    fn should_preserve_raw_bundle2(&self) -> bool {
        self.preserve_raw_bundle2
    }
}

pub fn gettreepack_entries(
    ctx: CoreContext,
    repo: &BlobRepo,
    params: GettreepackArgs,
) -> BoxStream<(Box<Entry + Sync>, Option<MPath>), Error> {
    if !params.directories.is_empty() {
        // This param is not used by core hg, don't worry about implementing it now
        return stream::once(Err(err_msg("directories param is not supported"))).boxify();
    }

    let GettreepackArgs {
        rootdir,
        mfnodes,
        basemfnodes,
        depth: fetchdepth,
        directories: _,
    } = params;

    // 65536 matches the default TREE_DEPTH_MAX value from Mercurial
    let fetchdepth = fetchdepth.unwrap_or(2 << 16);

    // TODO(stash): T25850889 only one basemfnodes is used. That means that trees that client
    // already has can be sent to the client.
    let mut basemfnode = basemfnodes.iter().next().cloned();

    let rootpath = if rootdir.is_empty() {
        None
    } else {
        Some(try_boxstream!(MPath::new(rootdir)))
    };

    select_all(
        mfnodes
            .iter()
            .filter(|node| !basemfnodes.contains(node))
            .map(move |mfnode| {
                let cur_basemfnode = basemfnode.unwrap_or(HgManifestId::new(NULL_HASH));
                // `basemfnode`s are used to reduce the data we send the client by having us prune
                // manifests the client already has. If the client claims to have no manifests,
                // then give it a full set for the first manifest it requested, then give it diffs
                // against the manifest we now know it has (the one we're sending), to reduce
                // the data we send.
                if basemfnode.is_none() {
                    basemfnode = Some(*mfnode);
                }

                get_changed_manifests_stream(
                    ctx.clone(),
                    repo,
                    *mfnode,
                    cur_basemfnode,
                    rootpath.clone(),
                    CombinatorPruner::new(FilePruner, DeletedPruner),
                    fetchdepth,
                )
            }),
    )
    .boxify()
}

fn get_changed_manifests_stream(
    ctx: CoreContext,
    repo: &BlobRepo,
    mfid: HgManifestId,
    basemfid: HgManifestId,
    rootpath: Option<MPath>,
    pruner: impl Pruner + Send + Clone + 'static,
    max_depth: usize,
) -> BoxStream<(Box<Entry + Sync>, Option<MPath>), Error> {
    let manifest = repo.get_manifest_by_nodeid(ctx.clone(), mfid);
    let basemanifest = repo.get_manifest_by_nodeid(ctx.clone(), basemfid);
    let entry: Box<Entry + Sync> = Box::new(repo.get_root_entry(mfid));
    let root_entry_stream = stream::once(Ok((entry, rootpath.clone())));

    if max_depth == 1 {
        return root_entry_stream.boxify();
    }

    let changed_entries = manifest
        .join(basemanifest)
        .map({
            cloned!(ctx, rootpath);
            move |(mf, basemf)| {
                changed_entry_stream_with_pruner(
                    ctx,
                    &mf,
                    &basemf,
                    rootpath,
                    pruner,
                    Some(max_depth),
                )
            }
        })
        .flatten_stream();

    let changed_entries = changed_entries.map(move |entry_status| match entry_status.status {
        EntryStatus::Added(to_entry) | EntryStatus::Modified { to_entry, .. } => {
            assert!(
                to_entry.get_type() == Type::Tree,
                "FilePruner should have removed file entries"
            );
            (to_entry, entry_status.dirname)
        }
        EntryStatus::Deleted(..) => {
            panic!("DeletedPruner should have removed deleted entries");
        }
    });

    // Append root manifest as well
    changed_entries.chain(root_entry_stream).boxify()
}

fn fetch_treepack_part_input(
    ctx: CoreContext,
    repo: &BlobRepo,
    entry: Box<Entry + Sync>,
    basepath: Option<MPath>,
    validate_content: bool,
) -> BoxFuture<parts::TreepackPartInput, Error> {
    let path = MPath::join_element_opt(basepath.as_ref(), entry.get_name());
    let repo_path = match path {
        Some(path) => {
            if entry.get_type() == Type::Tree {
                RepoPath::DirectoryPath(path)
            } else {
                RepoPath::FilePath(path)
            }
        }
        None => RepoPath::RootPath,
    };

    let node = entry.get_hash().clone();
    let path = repo_path.clone();

    let parents = entry.get_parents(ctx.clone());

    let linknode_fut = repo.get_linknode_opt(
        ctx.clone(),
        &repo_path,
        HgFileNodeId::new(entry.get_hash().into_nodehash()),
    );

    let content_fut = entry
        .get_raw_content(ctx.clone())
        .map(|blob| blob.into_inner());

    let validate_content = if validate_content {
        entry
            .get_raw_content(ctx.clone())
            .join(entry.get_parents(ctx.clone()))
            .and_then(move |(content, parents)| {
                let (p1, p2) = parents.get_nodes();
                let actual = node.into_nodehash();
                // Do not do verification for a root node because it might be broken
                // because of migration to tree manifest.
                let expected = HgBlobNode::new(content, p1, p2).nodeid();
                if path.is_root() || actual == expected {
                    Ok(())
                } else {
                    let error_msg = format!(
                        "gettreepack: {} expected: {} actual: {}",
                        path, expected, actual
                    );
                    ctx.scuba()
                        .clone()
                        .log_with_msg("Data corruption", Some(error_msg));
                    Err(ErrorKind::DataCorruption {
                        path,
                        expected,
                        actual,
                    }
                    .into())
                }
            })
            .left_future()
    } else {
        future::ok(()).right_future()
    };

    parents
        .join(linknode_fut)
        .join(content_fut)
        .join(validate_content)
        .map(|(val, ())| val)
        .map(move |((parents, linknode_opt), content)| {
            let (p1, p2) = parents.get_nodes();
            parts::TreepackPartInput {
                node: node.into_nodehash(),
                p1,
                p2,
                content,
                name: entry.get_name().cloned(),
                linknode: linknode_opt.unwrap_or(NULL_CSID).into_nodehash(),
                basepath,
            }
        })
        .boxify()
}

/// getbundle capabilities have tricky format.
/// It has a few layers of encoding. Upper layer is a key value pair in format `key=value`,
/// value can be empty and '=' may not be there. If it's not empty then it's urlencoded list
/// of chunks separated with '\n'. Each chunk is in a format 'key=value1,value2...' where both
/// `key` and `value#` are url encoded. Again, values can be empty, '=' might not be there
fn parse_utf8_getbundle_caps(caps: &[u8]) -> Option<(String, HashMap<String, HashSet<String>>)> {
    match caps.iter().position(|&x| x == b'=') {
        Some(pos) => {
            let (name, urlencodedcap) = caps.split_at(pos);
            // Skip the '='
            let urlencodedcap = &urlencodedcap[1..];
            let name = String::from_utf8(name.to_vec()).ok()?;

            let mut ans = HashMap::new();
            let caps = percent_encoding::percent_decode(urlencodedcap)
                .decode_utf8()
                .ok()?;
            for cap in caps.split('\n') {
                let split = cap.splitn(2, '=').collect::<Vec<_>>();
                let urlencoded_cap_name = split.get(0)?;
                let cap_name = percent_encoding::percent_decode(urlencoded_cap_name.as_bytes())
                    .decode_utf8()
                    .ok()?;
                let mut values = HashSet::new();

                if let Some(urlencoded_values) = split.get(1) {
                    for urlencoded_value in urlencoded_values.split(',') {
                        let value = percent_encoding::percent_decode(urlencoded_value.as_bytes());
                        let value = value.decode_utf8().ok()?;
                        values.insert(value.to_string());
                    }
                }
                ans.insert(cap_name.to_string(), values);
            }

            Some((name, ans))
        }
        None => String::from_utf8(caps.to_vec())
            .map(|cap| (cap, HashMap::new()))
            .ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maplit::hashset;

    #[test]
    fn test_parsing_caps_simple() {
        assert_eq!(
            parse_utf8_getbundle_caps(b"cap"),
            Some((String::from("cap"), HashMap::new())),
        );

        let caps = b"bundle2=HG20";

        assert_eq!(
            parse_utf8_getbundle_caps(caps),
            Some((
                String::from("bundle2"),
                hashmap! { "HG20".to_string() => hashset!{} }
            )),
        );

        let caps = b"bundle2=HG20%0Ab2x%253Ainfinitepush%0Ab2x%253Ainfinitepushscratchbookmarks\
        %0Ab2x%253Arebase%0Abookmarks%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0A\
        error%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0A\
        pushkey%0Aremote-changegroup%3Dhttp%2Chttps%0Aremotefilelog%3DTrue%0Atreemanifest%3DTrue%0Atreeonly%3DTrue";

        assert_eq!(
            parse_utf8_getbundle_caps(caps),
            Some((
                String::from("bundle2"),
                hashmap! {
                    "HG20".to_string() => hashset!{},
                    "b2x:rebase".to_string() => hashset!{},
                    "digests".to_string() => hashset!{"md5".to_string(), "sha512".to_string(), "sha1".to_string()},
                    "listkeys".to_string() => hashset!{},
                    "remotefilelog".to_string() => hashset!{"True".to_string()},
                    "hgtagsfnodes".to_string() => hashset!{},
                    "bookmarks".to_string() => hashset!{},
                    "b2x:infinitepushscratchbookmarks".to_string() => hashset!{},
                    "treeonly".to_string() => hashset!{"True".to_string()},
                    "pushkey".to_string() => hashset!{},
                    "error".to_string() => hashset!{
                        "pushraced".to_string(),
                        "pushkey".to_string(),
                        "unsupportedcontent".to_string(),
                        "abort".to_string(),
                    },
                    "b2x:infinitepush".to_string() => hashset!{},
                    "changegroup".to_string() => hashset!{"01".to_string(), "02".to_string()},
                    "remote-changegroup".to_string() => hashset!{"http".to_string(), "https".to_string()},
                    "treemanifest".to_string() => hashset!{"True".to_string()},
                }
            )),
        );
    }

}
