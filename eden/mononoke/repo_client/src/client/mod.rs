/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::errors::ErrorKind;

use unbundle::{
    run_hooks, run_post_resolve_action, BundleResolverError, PushRedirector, PushRedirectorArgs,
};

use anyhow::{format_err, Error, Result};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bookmarks::{Bookmark, BookmarkName, BookmarkPrefix};
use bookmarks_types::BookmarkKind;
use bytes::Bytes;
use bytes_old::{BufMut as BufMutOld, Bytes as BytesOld, BytesMut as BytesMutOld};
use cloned::cloned;
use context::{CoreContext, LoggingContainer, PerfCounterType, SessionContainer};
use filenodes::FilenodeResult;
use futures::{
    channel::oneshot::{self, Sender},
    compat::Future01CompatExt,
    future::{self, select, Either, FutureExt, TryFutureExt},
    pin_mut,
};
use futures_ext::{
    spawn_future, try_boxfuture, try_boxstream, BoxFuture, BoxStream, BufferedParams,
    FutureExt as OldFutureExt, StreamExt, StreamTimeoutError,
};
use futures_old::future::ok;
use futures_old::{
    future as future_old, stream, try_ready, Async, Future, IntoFuture, Poll, Stream,
};
use futures_stats::{Timed, TimedStreamTrait};
use getbundle_response::{
    create_getbundle_response, DraftsInBundlesPolicy, PhasesPart, SessionLfsParams,
};
use hgproto::{GetbundleArgs, GettreepackArgs, HgCommandRes, HgCommands};
use hostname::get_hostname;
use itertools::Itertools;
use lazy_static::lazy_static;
use live_commit_sync_config::{CfgrLiveCommitSyncConfig, LiveCommitSyncConfig};
use load_limiter::Metric;
use manifest::{Diff, Entry, ManifestOps};
use maplit::hashmap;
use mercurial_bundles::{create_bundle_stream, parts, wirepack, Bundle2Item};
use mercurial_revlog::{self, RevlogChangeset};
use mercurial_types::{
    blobs::HgBlobChangeset, calculate_hg_node_id, convert_parents_to_remotefilelog_format,
    fetch_manifest_envelope, percent_encode, Delta, HgChangesetId, HgChangesetIdPrefix,
    HgChangesetIdsResolvedFromPrefix, HgFileNodeId, HgManifestId, HgNodeHash, HgParents, MPath,
    RepoPath, NULL_CSID, NULL_HASH,
};
use metaconfig_types::{RepoClientKnobs, RepoReadOnly};
use mononoke_repo::{MononokeRepo, SqlStreamingCloneConfig};
use rand::{self, Rng};
use remotefilelog::{
    create_getpack_v1_blob, create_getpack_v2_blob, get_unordered_file_history_for_multiple_nodes,
};
use revisionstore_types::Metadata;
use scuba_ext::ScubaSampleBuilderExt;
use serde_json::{self, json};
use slog::{debug, info, o};
use stats::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::TryInto;
use std::fmt::Write;
use std::mem;
use std::str::FromStr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use streaming_clone::RevlogStreamingChunks;
use time_ext::DurationExt;
use tokio::time::delay_for;
use tokio_old::timer::timeout::Error as TimeoutError;
use tokio_old::util::FutureExt as TokioFutureExt;
use tracing::{trace_args, Traced};
use tunables::tunables;
use warm_bookmarks_cache::WarmBookmarksCache;

mod logging;
mod monitor;
mod session_bookmarks_cache;

use logging::CommandLogger;
pub use logging::WireprotoLogging;
use monitor::Monitor;
use session_bookmarks_cache::SessionBookmarkCache;

define_stats! {
    prefix = "mononoke.repo_client";
    getbundle_ms:
        histogram(10, 0, 1_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    gettreepack_ms:
        histogram(2, 0, 200, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    getpack_ms:
        histogram(20, 0, 2_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    getcommitdata_ms:
        histogram(2, 0, 200, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    total_tree_count: timeseries(Rate, Sum),
    quicksand_tree_count: timeseries(Rate, Sum),
    total_tree_size: timeseries(Rate, Sum),
    quicksand_tree_size: timeseries(Rate, Sum),
    total_fetched_file_size: timeseries(Rate, Sum),
    quicksand_fetched_file_size: timeseries(Rate, Sum),
    null_linknode_gettreepack: timeseries(Rate, Sum),
    null_linknode_getpack: timeseries(Rate, Sum),
    getcommitdata_commit_count: timeseries(Rate, Sum),

    push_success: dynamic_timeseries("push_success.{}", (reponame: String); Rate, Sum),
    push_hook_failure: dynamic_timeseries("push_hook_failure.{}.{}", (reponame: String, hook_failure: String); Rate, Sum),
    push_conflicts: dynamic_timeseries("push_conflicts.{}", (reponame: String); Rate, Sum),
    rate_limits_exceeded: dynamic_timeseries("rate_limits_exceeded.{}", (reponame: String); Rate, Sum),
    push_error: dynamic_timeseries("push_error.{}", (reponame: String); Rate, Sum),

    undesired_tree_fetches: timeseries(Sum),
    undesired_file_fetches: timeseries(Sum),
    undesired_file_fetches_sizes: timeseries(Sum),
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
    pub static GETPACKV1: &str = "getpackv1";
    pub static GETPACKV2: &str = "getpackv2";
    pub static STREAMOUTSHALLOW: &str = "stream_out_shallow";
    pub static GETCOMMITDATA: &str = "getcommitdata";
}

fn debug_format_path(path: &Option<MPath>) -> String {
    match path {
        Some(p) => format!("{}", p),
        None => String::new(),
    }
}

fn debug_format_nodes<'a>(nodes: impl IntoIterator<Item = &'a HgChangesetId>) -> String {
    nodes.into_iter().map(|node| format!("{}", node)).join(" ")
}

fn debug_format_manifests<'a>(nodes: impl IntoIterator<Item = &'a HgManifestId>) -> String {
    nodes.into_iter().map(|node| format!("{}", node)).join(" ")
}

fn debug_format_directories<'a, T: AsRef<[u8]> + 'a>(
    directories: impl IntoIterator<Item = &'a T>,
) -> String {
    let encoded_directories = directories
        .into_iter()
        .map(hgproto::batch::escape)
        .collect::<Vec<_>>();

    let len = encoded_directories
        .iter()
        .map(|v| v.len())
        .fold(0, |sum, len| sum + len + 1);

    let mut out = Vec::with_capacity(len);

    for vec in encoded_directories {
        out.extend(vec);
        out.extend(b",");
    }

    // NOTE: This normally shouldn't happen, but this is just a debug function, so if it does we
    // just ignore it.
    String::from_utf8_lossy(out.as_ref()).to_string()
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

lazy_static! {
    static ref TIMEOUT: Duration = Duration::from_secs(15 * 60);
    // Bookmarks taking a long time is unexpected and bad - limit them specially
    static ref BOOKMARKS_TIMEOUT: Duration = Duration::from_secs(3 * 60);
    // getbundle requests can be very slow for huge commits
    static ref GETBUNDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
    // clone requests can be rather long. Let's bump the timeout
    static ref CLONE_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);
    // getfiles requests can be rather long. Let's bump the timeout
    static ref GETPACK_TIMEOUT: Duration = Duration::from_secs(90 * 60);
    static ref LOAD_LIMIT_TIMEFRAME: Duration = Duration::from_secs(1);
    static ref SLOW_REQUEST_THRESHOLD: Duration = Duration::from_secs(1);
}

pub(crate) fn process_timeout_error(err: TimeoutError<Error>) -> Error {
    match err.into_inner() {
        Some(err) => err,
        None => Error::msg("timeout"),
    }
}

fn process_stream_timeout_error(err: StreamTimeoutError) -> Error {
    match err {
        StreamTimeoutError::Error(err) => err,
        StreamTimeoutError::Timeout => Error::msg("timeout"),
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
        "designatednodes".to_string(),
        "getcommitdata".to_string(),
    ]
}

fn bundle2caps() -> String {
    let caps = {
        let mut caps = vec![
            ("HG20", vec![]),
            ("changegroup", vec!["02", "03"]),
            ("b2x:infinitepush", vec![]),
            ("b2x:infinitepushscratchbookmarks", vec![]),
            ("pushkey", vec![]),
            ("treemanifestserver", vec!["True"]),
            ("b2x:rebase", vec![]),
            ("b2x:rebasepackpart", vec![]),
            ("phases", vec!["heads"]),
            ("obsmarkers", vec!["V1"]),
            ("listkeys", vec![]),
        ];

        if tunables().get_mutation_advertise_for_infinitepush() {
            caps.push(("b2x:infinitepushmutation", vec![]));
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

struct UndesiredPathLogger {
    ctx: CoreContext,
    repo_needs_logging: bool,
    path_prefix_to_log: Option<MPath>,
}

impl UndesiredPathLogger {
    fn new(ctx: CoreContext, repo: &BlobRepo) -> Result<Self, Error> {
        let tunables = tunables();
        let repo_needs_logging =
            repo.name() == tunables.get_undesired_path_repo_name_to_log().as_str();

        let path_prefix_to_log = if repo_needs_logging {
            MPath::new_opt(tunables.get_undesired_path_prefix_to_log().as_str())?
        } else {
            None
        };

        Ok(Self {
            ctx,
            repo_needs_logging,
            path_prefix_to_log,
        })
    }

    fn maybe_log_tree(&self, path: Option<&MPath>) {
        if self.should_log(path) {
            STATS::undesired_tree_fetches.add_value(1);
            self.ctx
                .perf_counters()
                .add_to_counter(PerfCounterType::UndesiredTreeFetch, 1);
        }
    }

    fn maybe_log_file(&self, path: Option<&MPath>, sizes: impl Iterator<Item = u64>) {
        if self.should_log(path) {
            for size in sizes {
                STATS::undesired_file_fetches.add_value(1);
                STATS::undesired_file_fetches_sizes.add_value(size as i64);
                self.ctx
                    .perf_counters()
                    .add_to_counter(PerfCounterType::UndesiredFileFetch, 1);
                self.ctx
                    .perf_counters()
                    .add_to_counter(PerfCounterType::UndesiredFileFetchSize, size as i64);

                self.ctx
                    .scuba()
                    .clone()
                    .add("undesired_file_size", size)
                    .log_with_msg("Undesired file fetch", format!("{:?}", path));
            }
        }
    }

    fn should_log(&self, path: Option<&MPath>) -> bool {
        if self.repo_needs_logging {
            MPath::is_prefix_of_opt(self.path_prefix_to_log.as_ref(), MPath::iter_opt(path))
        } else {
            false
        }
    }
}

#[derive(Clone)]
pub struct RepoClient {
    repo: MononokeRepo,
    // The session for this repo access.
    session: SessionContainer,
    // A base logging container. This will be combined with the Session container for each command
    // to produce a CoreContext.
    logging: LoggingContainer,
    // Percent of returned entries (filelogs, manifests, changesets) which content
    // will be hash validated
    hash_validation_percentage: usize,
    // Whether to save raw bundle2 content into the blobstore
    preserve_raw_bundle2: bool,
    // There is a race condition in bookmarks handling in Mercurial, which needs protocol-level
    // fixes. See `test-bookmark-race.t` for a reproducer; the issue is that between discovery
    // and bookmark handling (listkeys), we can get new commits and a bookmark change.
    // The client then gets a bookmark that points to a commit it does not yet have, and ignores it.
    // We currently fix it by caching bookmarks at the beginning of discovery.
    // TODO: T45411456 Fix this by teaching the client to expect extra commits to correspond to the bookmarks.
    session_bookmarks_cache: Arc<SessionBookmarkCache>,
    wireproto_logging: Arc<WireprotoLogging>,
    maybe_push_redirector_args: Option<PushRedirectorArgs>,
    force_lfs: Arc<AtomicBool>,
    maybe_live_commit_sync_config: Option<CfgrLiveCommitSyncConfig>,
    knobs: RepoClientKnobs,
}

impl RepoClient {
    pub fn new(
        repo: MononokeRepo,
        session: SessionContainer,
        logging: LoggingContainer,
        hash_validation_percentage: usize,
        preserve_raw_bundle2: bool,
        wireproto_logging: Arc<WireprotoLogging>,
        maybe_push_redirector_args: Option<PushRedirectorArgs>,
        maybe_live_commit_sync_config: Option<CfgrLiveCommitSyncConfig>,
        maybe_warm_bookmarks_cache: Option<Arc<WarmBookmarksCache>>,
        knobs: RepoClientKnobs,
    ) -> Self {
        let blobrepo = repo.blobrepo().clone();
        Self {
            repo,
            session,
            logging,
            hash_validation_percentage,
            preserve_raw_bundle2,
            session_bookmarks_cache: Arc::new(SessionBookmarkCache::new(
                blobrepo,
                maybe_warm_bookmarks_cache,
            )),
            wireproto_logging,
            maybe_push_redirector_args,
            force_lfs: Arc::new(AtomicBool::new(false)),
            maybe_live_commit_sync_config,
            knobs,
        }
    }

    fn command_future<F, I, E, H>(&self, command: &str, handler: H) -> BoxFuture<I, E>
    where
        F: Future<Item = I, Error = E> + Send + 'static,
        H: FnOnce(CoreContext, CommandLogger) -> F,
    {
        let (ctx, command_logger) = self.start_command(command);
        with_command_monitor(ctx.clone(), handler(ctx, command_logger)).boxify()
    }

    fn command_stream<S, I, E, H>(&self, command: &str, handler: H) -> BoxStream<I, E>
    where
        S: Stream<Item = I, Error = E> + Send + 'static,
        H: FnOnce(CoreContext, CommandLogger) -> S,
    {
        let (ctx, command_logger) = self.start_command(command);
        with_command_monitor(ctx.clone(), handler(ctx, command_logger)).boxify()
    }

    fn start_command(&self, command: &str) -> (CoreContext, CommandLogger) {
        info!(self.logging.logger(), "{}", command);

        let logger = self
            .logging
            .logger()
            .new(o!("command" => command.to_owned()));

        let mut scuba = self.logging.scuba().clone();
        scuba.add("command", command);
        scuba.log_with_msg("Start processing", None);

        let ctx =
            self.session
                .new_context_with_scribe(logger, scuba, self.logging.scribe().clone());

        let command_logger = CommandLogger::new(
            ctx.clone(),
            command.to_owned(),
            self.wireproto_logging.clone(),
        );

        (ctx, command_logger)
    }

    fn get_publishing_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = HashMap<Bookmark, HgChangesetId>, Error = Error> {
        self.session_bookmarks_cache.get_publishing_bookmarks(ctx)
    }

    fn get_pull_default_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = HashMap<Vec<u8>, Vec<u8>>, Error = Error> {
        self.get_publishing_bookmarks_maybe_stale(ctx)
            .map(|bookmarks| {
                bookmarks
                    .into_iter()
                    .filter_map(|(book, cs)| {
                        let hash: Vec<u8> = cs.into_nodehash().to_hex().into();
                        if book.kind() == &BookmarkKind::PullDefaultPublishing {
                            Some((book.into_name().into_byte_vec(), hash))
                        } else {
                            None
                        }
                    })
                    .collect()
            })
    }

    fn create_bundle(&self, ctx: CoreContext, args: GetbundleArgs) -> BoxStream<BytesOld, Error> {
        let lfs_params = self.lfs_params();
        let blobrepo = self.repo.blobrepo().clone();
        let reponame = self.repo.reponame().clone();
        let mut bundle2_parts = vec![];

        let GetbundleArgs {
            bundlecaps,
            common,
            heads,
            phases,
            listkeys,
        } = args;

        let mut use_phases = phases;
        if use_phases {
            for cap in &bundlecaps {
                if let Some((cap_name, caps)) = parse_utf8_getbundle_caps(cap) {
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
        let pull_default_bookmarks = self.get_pull_default_bookmarks_maybe_stale(ctx.clone());
        let lca_hint = self.repo.lca_hint().clone();

        let drafts_in_bundles_policy = if self.repo.infinitepush().hydrate_getbundle_response {
            DraftsInBundlesPolicy::WithTreesAndFiles
        } else {
            DraftsInBundlesPolicy::CommitsOnly
        };

        async move {
            create_getbundle_response(
                ctx.clone(),
                blobrepo.clone(),
                reponame,
                common,
                heads,
                lca_hint,
                if use_phases {
                    PhasesPart::Yes
                } else {
                    PhasesPart::No
                },
                lfs_params,
                drafts_in_bundles_policy,
            )
            .await
        }
        .boxed()
        .compat()
        .and_then(move |mut getbundle_response| {
            bundle2_parts.append(&mut getbundle_response);

            // listkeys bookmarks part is added separately.

            // TODO: generalize this to other listkey types
            // (note: just calling &b"bookmarks"[..] doesn't work because https://fburl.com/0p0sq6kp)
            if listkeys.contains(&b"bookmarks".to_vec()) {
                let items = pull_default_bookmarks
                    .map(|bookmarks| stream::iter_ok(bookmarks))
                    .flatten_stream();
                bundle2_parts.push(parts::listkey_part("bookmarks", items)?);
            }
            // TODO(stash): handle includepattern= and excludepattern=

            let compression = None;
            Ok(create_bundle_stream(bundle2_parts, compression).boxify())
        })
        .flatten_stream()
        .boxify()
    }

    fn gettreepack_untimed(
        &self,
        ctx: CoreContext,
        params: GettreepackArgs,
    ) -> BoxStream<BytesOld, Error> {
        let validate_hash = rand::random::<usize>() % 100 < self.hash_validation_percentage;

        let undesired_path_logger =
            try_boxstream!(UndesiredPathLogger::new(ctx.clone(), self.repo.blobrepo()));

        let changed_entries = gettreepack_entries(ctx.clone(), self.repo.blobrepo(), params)
            .filter({
                let mut used_hashes = HashSet::new();
                move |(hg_mf_id, _)| used_hashes.insert(hg_mf_id.clone())
            })
            .map({
                cloned!(ctx);
                let blobrepo = self.repo.blobrepo().clone();
                move |(hg_mf_id, path)| {
                    undesired_path_logger.maybe_log_tree(path.as_ref());

                    ctx.perf_counters()
                        .increment_counter(PerfCounterType::GettreepackNumTreepacks);

                    ctx.session().bump_load(Metric::EgressTotalManifests, 1.0);
                    STATS::total_tree_count.add_value(1);
                    if ctx.session().is_quicksand() {
                        STATS::quicksand_tree_count.add_value(1);
                    }
                    fetch_treepack_part_input(ctx.clone(), &blobrepo, hg_mf_id, path, validate_hash)
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

    fn getpack<WeightedContent, Content, GetpackHandler>(
        &self,
        params: BoxStream<(MPath, Vec<HgFileNodeId>), Error>,
        handler: GetpackHandler,
        name: &'static str,
    ) -> BoxStream<BytesOld, Error>
    where
        WeightedContent: Future<Item = (u64, Content), Error = Error> + Send + 'static,
        Content:
            Future<Item = (HgFileNodeId, Bytes, Option<Metadata>), Error = Error> + Send + 'static,
        GetpackHandler: Fn(CoreContext, BlobRepo, HgFileNodeId, SessionLfsParams, bool) -> WeightedContent
            + Send
            + 'static,
    {
        let allow_short_getpack_history = self.knobs.allow_short_getpack_history;
        self.command_stream(name, |ctx, command_logger| {
            let undesired_path_logger =
                try_boxstream!(UndesiredPathLogger::new(ctx.clone(), self.repo.blobrepo()));
            let undesired_path_logger = Arc::new(undesired_path_logger);
            // We buffer all parameters in memory so that we can log them.
            // That shouldn't be a problem because requests are quite small
            let getpack_params = Arc::new(Mutex::new(vec![]));
            let repo = self.repo.blobrepo().clone();

            let lfs_params = self.lfs_params();

            let validate_hash =
                rand::thread_rng().gen_ratio(self.hash_validation_percentage as u32, 100);
            let getpack_buffer_size = 500;
            // Let's fetch the whole request before responding.
            // That's prevents deadlocks, because hg client doesn't start reading the response
            // before all the arguments were sent.
            let request_stream = move || {
                cloned!(ctx);
                let s = params
                    .collect()
                    .map({
                        cloned!(ctx);
                        move |params| {
                            ctx.scuba()
                                .clone()
                                .add("getpack_paths", params.len())
                                .log_with_msg("Getpack Params", None);
                            stream::iter_ok(params.into_iter())
                        }
                    })
                    .flatten_stream()
                    .map({
                        cloned!(ctx, getpack_params, repo, lfs_params);
                        move |(path, filenodes)| {
                            {
                                let mut getpack_params = getpack_params.lock().unwrap();
                                getpack_params.push((path.clone(), filenodes.clone()));
                            }

                            ctx.session().bump_load(Metric::EgressGetpackFiles, 1.0);

                            let blob_futs: Vec<_> = filenodes
                                .iter()
                                .map(|filenode| {
                                    handler(
                                        ctx.clone(),
                                        repo.clone(),
                                        *filenode,
                                        lfs_params.clone(),
                                        validate_hash,
                                    )
                                })
                                .collect();

                            // NOTE: We don't otherwise await history_fut until we have the results
                            // from blob_futs, so we need to spawn this to start fetching history
                            // before we have resoved hg filenodes.
                            let history_fut = spawn_future(
                                get_unordered_file_history_for_multiple_nodes(
                                    ctx.clone(),
                                    repo.clone(),
                                    filenodes.into_iter().collect(),
                                    &path,
                                    allow_short_getpack_history,
                                )
                                .collect(),
                            );

                            future_old::join_all(blob_futs.into_iter()).map({
                                cloned!(undesired_path_logger);
                                move |blobs| {
                                    undesired_path_logger.maybe_log_file(
                                        Some(&path),
                                        blobs.iter().map(|(size, _)| *size),
                                    );

                                    let total_weight = blobs.iter().map(|(size, _)| size).sum();
                                    let content_futs = blobs.into_iter().map(|(_, fut)| fut);
                                    let contents_and_history = future_old::join_all(content_futs)
                                        .join(history_fut)
                                        .map(move |(contents, history)| (path, contents, history));

                                    (contents_and_history, total_weight)
                                }
                            })
                        }
                    })
                    .buffered(getpack_buffer_size);

                let params = BufferedParams {
                    weight_limit: 100_000_000,
                    buffer_size: getpack_buffer_size,
                };
                let s = s
                    .buffered_weight_limited(params)
                    .whole_stream_timeout(*GETPACK_TIMEOUT)
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
                                let linknode = history_entry.linknode().into_nodehash();
                                if linknode == NULL_HASH {
                                    ctx.perf_counters()
                                        .increment_counter(PerfCounterType::NullLinknode);
                                    STATS::null_linknode_getpack.add_value(1);
                                }

                                wirepack::Part::History(wirepack::HistoryEntry {
                                    node: history_entry.filenode().into_nodehash(),
                                    p1: p1.into_nodehash(),
                                    p2: p2.into_nodehash(),
                                    linknode,
                                    copy_from: copy_from.cloned().map(RepoPath::FilePath),
                                })
                            });
                            res.extend(history);

                            res.push(wirepack::Part::DataMeta {
                                path: RepoPath::FilePath(path),
                                entry_count: contents.len() as u32,
                            });
                            for (filenode, content, metadata) in contents {
                                let content = content.to_vec();
                                let length = content.len() as u64;

                                ctx.perf_counters().set_max_counter(
                                    PerfCounterType::GetpackMaxFileSize,
                                    length as i64,
                                );

                                if let Some(lfs_threshold) = lfs_params.threshold {
                                    if length >= lfs_threshold {
                                        ctx.perf_counters().add_to_counter(
                                            PerfCounterType::GetpackPossibleLFSFilesSumSize,
                                            length as i64,
                                        );

                                        ctx.perf_counters().increment_counter(
                                            PerfCounterType::GetpackNumPossibleLFSFiles,
                                        );
                                    }
                                }

                                res.push(wirepack::Part::Data(wirepack::DataEntry {
                                    node: filenode.into_nodehash(),
                                    delta_base: NULL_HASH,
                                    delta: Delta::new_fulltext(content),
                                    metadata,
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
                        cloned!(ctx);
                        move |bytes| {
                            let len = bytes.len() as i64;
                            ctx.perf_counters()
                                .add_to_counter(PerfCounterType::GetpackResponseSize, len);

                            STATS::total_fetched_file_size.add_value(len as i64);
                            if ctx.session().is_quicksand() {
                                STATS::quicksand_fetched_file_size.add_value(len as i64);
                            }
                        }
                    })
                    .timed({
                        cloned!(ctx);
                        move |stats, _| {
                            STATS::getpack_ms
                                .add_value(stats.completion_time.as_millis_unchecked() as i64);
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

                            ctx.perf_counters().add_to_counter(
                                PerfCounterType::GetpackNumFiles,
                                encoded_params.len() as i64,
                            );

                            command_logger.finalize_command(
                                ctx,
                                &stats,
                                Some(&json! {encoded_params}),
                            );

                            Ok(())
                        }
                    })
            };

            throttle_stream(
                &self.session,
                Metric::EgressGetpackFiles,
                name,
                request_stream,
            )
            .boxify()
        })
    }

    fn lfs_params(&self) -> SessionLfsParams {
        if self.force_lfs.load(Ordering::Relaxed) {
            self.repo.force_lfs_if_threshold_set()
        } else {
            self.repo
                .lfs_params(self.session.source_hostname().as_deref())
        }
    }

    fn maybe_get_pushredirector_for_action(
        &self,
        ctx: &CoreContext,
        action: &unbundle::PostResolveAction,
    ) -> Result<Option<PushRedirector>> {
        let push_redirector_args = match self.maybe_push_redirector_args.clone() {
            Some(push_redirector_args) => push_redirector_args,
            None => {
                debug!(
                    ctx.logger(),
                    "maybe_push_redirector_args are none, no push_redirector for unbundle"
                );
                return Ok(None);
            }
        };

        match self.maybe_live_commit_sync_config {
            None => Ok(None),
            Some(ref live_commit_sync_config) => {
                use unbundle::PostResolveAction::*;

                let repo_id = self.repo.blobrepo().get_repoid();
                let redirect = match action {
                    InfinitePush(_) => {
                        live_commit_sync_config.push_redirector_enabled_for_draft(repo_id)
                    }
                    Push(_) | PushRebase(_) | BookmarkOnlyPushRebase(_) => {
                        live_commit_sync_config.push_redirector_enabled_for_public(repo_id)
                    }
                };

                if redirect {
                    debug!(
                        ctx.logger(),
                        "live_commit_sync_config says push redirection is on"
                    );
                    Ok(Some(push_redirector_args.into_push_redirector(
                        ctx,
                        &self.maybe_live_commit_sync_config,
                    )?))
                } else {
                    debug!(
                        ctx.logger(),
                        "live_commit_sync_config says push redirection is off"
                    );
                    Ok(None)
                }
            }
        }
    }
}

fn throttle_stream<F, S, V>(
    session: &SessionContainer,
    metric: Metric,
    name: &'static str,
    func: F,
) -> impl Stream<Item = V, Error = Error>
where
    F: FnOnce() -> S + Send + 'static,
    S: Stream<Item = V, Error = Error> + Send + 'static,
{
    let session = session.clone();
    async move { session.should_throttle(metric, *LOAD_LIMIT_TIMEFRAME).await }
        .boxed()
        .compat()
        .then(move |throttle| match throttle {
            Ok(throttle) => {
                if throttle {
                    let err: Error = ErrorKind::RequestThrottled {
                        request_name: name.to_string(),
                    }
                    .into();
                    Err(err)
                } else {
                    Ok(func())
                }
            }
            Err(never_type) => never_type,
        })
        .flatten_stream()
}

async fn check_lock_repo(repo: MononokeRepo) -> Result<Bytes, BundleResolverError> {
    loop {
        match repo
            .readonly()
            .or_else(|_| {
                ok::<RepoReadOnly, Error>(RepoReadOnly::ReadOnly(
                    "Failed to fetch repo lock status".to_string(),
                ))
            })
            .compat()
            .await?
        {
            RepoReadOnly::ReadOnly(reason) => {
                let e = Error::from(ErrorKind::RepoReadOnly(reason));
                return Err(e.into());
            }
            RepoReadOnly::ReadWrite => delay_for(Duration::from_secs(1)).await,
        }
    }
}

impl HgCommands for RepoClient {
    // @wireprotocommand('between', 'pairs')
    fn between(
        &self,
        pairs: Vec<(HgChangesetId, HgChangesetId)>,
    ) -> HgCommandRes<Vec<Vec<HgChangesetId>>> {
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

        impl Stream for ParentStream<BoxFuture<HgBlobChangeset, Error>> {
            type Item = HgChangesetId;
            type Error = Error;

            fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
                if self.n == self.bottom || self.n.into_nodehash() == NULL_HASH {
                    return Ok(Async::Ready(None));
                }

                self.wait_cs = self.wait_cs.take().or_else(|| {
                    Some(
                        self.n
                            .load(self.ctx.clone(), self.repo.blobrepo().blobstore())
                            .compat()
                            .from_err()
                            .boxify(),
                    )
                });
                let cs = try_ready!(self.wait_cs.as_mut().unwrap().poll());
                self.wait_cs = None; // got it

                let p = cs.p1().unwrap_or(NULL_HASH);
                let prev_n = mem::replace(&mut self.n, HgChangesetId::new(p));

                Ok(Async::Ready(Some(prev_n)))
            }
        }

        self.command_future(ops::BETWEEN, |ctx, command_logger| {
            // TODO(jsgf): do pairs in parallel?
            // TODO: directly return stream of streams
            cloned!(self.repo);
            stream::iter_ok(pairs.into_iter())
                .and_then({
                    cloned!(ctx);
                    move |(top, bottom)| {
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
                    }
                })
                .collect()
                .timeout(*TIMEOUT)
                .map_err(process_timeout_error)
                .traced(self.session.trace(), ops::BETWEEN, trace_args!())
                .timed(move |stats, _| {
                    command_logger.without_wireproto().finalize_command(&stats);
                    Ok(())
                })
        })
    }

    // @wireprotocommand('clienttelemetry')
    fn clienttelemetry(&self, args: HashMap<Vec<u8>, Vec<u8>>) -> HgCommandRes<String> {
        self.command_future(ops::CLIENTTELEMETRY, |_ctx, mut command_logger| {
            let hostname = get_hostname().unwrap_or_else(|_| "<no hostname found>".to_owned());

            if let Some(client_correlator) = args.get(b"correlator" as &[u8]) {
                command_logger.add_scuba_extra(
                    "client_correlator",
                    String::from_utf8_lossy(client_correlator).into_owned(),
                );
            }

            if let Some(command) = args.get(b"command" as &[u8]) {
                command_logger.add_scuba_extra(
                    "hg_short_command",
                    String::from_utf8_lossy(command).into_owned(),
                );
            }

            if let Some(val) = args.get(b"wantslfspointers" as &[u8]) {
                if val == b"True" {
                    self.force_lfs.store(true, Ordering::Relaxed);
                }
            }

            future_old::ok(hostname)
                .timeout(*TIMEOUT)
                .map_err(process_timeout_error)
                .traced(self.session.trace(), ops::CLIENTTELEMETRY, trace_args!())
                .timed(move |stats, _| {
                    command_logger.without_wireproto().finalize_command(&stats);
                    Ok(())
                })
        })
    }

    // @wireprotocommand('heads')
    fn heads(&self) -> HgCommandRes<HashSet<HgChangesetId>> {
        // Get a stream of heads and collect them into a HashSet
        // TODO: directly return stream of heads
        self.command_future(ops::HEADS, |ctx, command_logger| {
            // Heads are all the commits that has a publishing bookmarks
            // that points to it.
            self.get_publishing_bookmarks_maybe_stale(ctx)
                .map(|map| map.into_iter().map(|(_, hg_cs_id)| hg_cs_id).collect())
                .timeout(*TIMEOUT)
                .map_err(process_timeout_error)
                .traced(self.session.trace(), ops::HEADS, trace_args!())
                .timed(move |stats, _| {
                    command_logger.without_wireproto().finalize_command(&stats);
                    Ok(())
                })
        })
    }

    // @wireprotocommand('lookup', 'key')
    fn lookup(&self, key: String) -> HgCommandRes<BytesOld> {
        fn generate_resp_buf(success: bool, message: &[u8]) -> BytesOld {
            let mut buf = BytesMutOld::with_capacity(message.len() + 3);
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

        // Generate positive response including HgChangesetId as hex.
        fn generate_changeset_resp_buf(csid: HgChangesetId) -> HgCommandRes<BytesOld> {
            Ok(generate_resp_buf(true, csid.to_hex().as_bytes()))
                .into_future()
                .boxify()
        }

        // Generate error response with the message including suggestions (commits info).
        // Suggestions are ordered by commit time (most recent first).
        fn generate_suggestions_resp_buf(
            ctx: CoreContext,
            repo: BlobRepo,
            suggestion_cids: Vec<HgChangesetId>,
        ) -> HgCommandRes<BytesOld> {
            let futs = suggestion_cids
                .into_iter()
                .map(|hg_csid| {
                    hg_csid
                        .load(ctx.clone(), repo.blobstore())
                        .compat()
                        .from_err()
                        .map(move |cs| (cs.to_string().into_bytes(), cs.time().clone()))
                })
                .collect::<Vec<_>>();

            future_old::join_all(futs)
                .map(|mut info_plus_date| {
                    info_plus_date.sort_by_key(|&(_, time)| time);
                    let mut infos = info_plus_date
                        .into_iter()
                        .map(|(info, _)| info)
                        .collect::<Vec<_>>();
                    infos.push(b"ambiguous identifier\nsuggestions are:\n".to_vec());
                    infos.reverse();
                    generate_resp_buf(false, &infos.join(&[b'\n'][..]))
                })
                .boxify()
        }

        // Controls how many suggestions to fetch in case of ambiguous outcome of prefix lookup.
        const MAX_NUMBER_OF_SUGGESTIONS_TO_FETCH: usize = 10;

        self.command_future(ops::LOOKUP, |ctx, command_logger| {
            let repo = self.repo.blobrepo().clone();

            // Resolves changeset or set of suggestions from the key (full hex hash or a prefix) if exist.
            // Note: `get_many_hg_by_prefix` works for the full hex hashes well but
            //       `changeset_exists` has better caching and is preferable for the full length hex hashes.
            let node_fut = match HgChangesetId::from_str(&key) {
                Ok(csid) => repo
                    .changeset_exists(ctx.clone(), csid)
                    .map(move |exists| {
                        if exists {
                            HgChangesetIdsResolvedFromPrefix::Single(csid)
                        } else {
                            HgChangesetIdsResolvedFromPrefix::NoMatch
                        }
                    })
                    .boxify(),
                Err(_) => match HgChangesetIdPrefix::from_str(&key) {
                    Ok(cs_prefix) => repo
                        .get_bonsai_hg_mapping()
                        .get_many_hg_by_prefix(
                            ctx.clone(),
                            repo.get_repoid(),
                            cs_prefix,
                            MAX_NUMBER_OF_SUGGESTIONS_TO_FETCH,
                        )
                        .boxify(),
                    Err(_) => ok(HgChangesetIdsResolvedFromPrefix::NoMatch).boxify(),
                },
            };

            // The lookup order:
            // If there is an exact commit match, return that even if the key is the prefix of the hash.
            // If there is a bookmark match, return that.
            // If there are suggestions, show them. This happens in case of ambiguous outcome of prefix lookup.
            // Otherwise, show an error.

            let bookmark = BookmarkName::new(&key).ok();
            let lookup_fut = node_fut
                .and_then(move |resolved_cids| {
                    use HgChangesetIdsResolvedFromPrefix::*;

                    // Describing the priority relative to bookmark presence for the key.
                    enum LookupOutcome {
                        HighPriority(HgCommandRes<BytesOld>),
                        LowPriority(HgCommandRes<BytesOld>),
                    };

                    let outcome = match resolved_cids {
                        Single(csid) => {
                            LookupOutcome::HighPriority(generate_changeset_resp_buf(csid))
                        }
                        Multiple(suggestion_cids) => {
                            LookupOutcome::LowPriority(generate_suggestions_resp_buf(
                                ctx.clone(),
                                repo.clone(),
                                suggestion_cids,
                            ))
                        }
                        TooMany(_) => LookupOutcome::LowPriority(
                            Ok(generate_resp_buf(
                                false,
                                format!("ambiguous identifier '{}'", key).as_bytes(),
                            ))
                            .into_future()
                            .boxify(),
                        ),
                        NoMatch => LookupOutcome::LowPriority(
                            Ok(generate_resp_buf(
                                false,
                                format!("{} not found", key).as_bytes(),
                            ))
                            .into_future()
                            .boxify(),
                        ),
                    };

                    match (outcome, bookmark) {
                        (LookupOutcome::HighPriority(res), _) => res,
                        (LookupOutcome::LowPriority(res), Some(bookmark)) => repo
                            .get_bookmark(ctx.clone(), &bookmark)
                            .and_then(move |maybe_csid| {
                                if let Some(csid) = maybe_csid {
                                    generate_changeset_resp_buf(csid)
                                } else {
                                    res
                                }
                            })
                            .boxify(),
                        (LookupOutcome::LowPriority(res), None) => res,
                    }
                })
                .boxify();

            lookup_fut
                .timeout(*TIMEOUT)
                .map_err(process_timeout_error)
                .traced(self.session.trace(), ops::LOOKUP, trace_args!())
                .timed(move |stats, _| {
                    command_logger.without_wireproto().finalize_command(&stats);
                    Ok(())
                })
        })
    }

    // @wireprotocommand('known', 'nodes *'), but the '*' is ignored
    fn known(&self, nodes: Vec<HgChangesetId>) -> HgCommandRes<Vec<bool>> {
        self.command_future(ops::KNOWN, |ctx, command_logger| {
            let blobrepo = self.repo.blobrepo().clone();

            let nodes_len = nodes.len();
            let phases_hint = blobrepo.get_phases().clone();

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
                .and_then({
                    cloned!(ctx);
                    move |(bcs_ids, bcs_hg_mapping)| {
                        phases_hint
                            .get_public(ctx, bcs_ids, false)
                            .map(move |public_csids| {
                                public_csids
                                    .into_iter()
                                    .filter_map(|csid| bcs_hg_mapping.get(&csid).cloned())
                                    .collect::<HashSet<_>>()
                            })
                    }
                })
                .map(move |found_hg_changesets| {
                    nodes
                        .into_iter()
                        .map(move |node| found_hg_changesets.contains(&node))
                        .collect::<Vec<_>>()
                })
                .timeout(*TIMEOUT)
                .map_err(process_timeout_error)
                .traced(self.session.trace(), ops::KNOWN, trace_args!())
                .timed(move |stats, known_nodes| {
                    if let Ok(known) = known_nodes {
                        ctx.perf_counters()
                            .add_to_counter(PerfCounterType::NumKnown, known.len() as i64);
                        ctx.perf_counters().add_to_counter(
                            PerfCounterType::NumUnknown,
                            (nodes_len - known.len()) as i64,
                        );
                    }
                    command_logger.without_wireproto().finalize_command(&stats);
                    Ok(())
                })
        })
    }

    fn knownnodes(&self, nodes: Vec<HgChangesetId>) -> HgCommandRes<Vec<bool>> {
        self.command_future(ops::KNOWNNODES, |ctx, command_logger| {
            let blobrepo = self.repo.blobrepo().clone();

            let nodes_len = nodes.len();

            blobrepo
                .get_hg_bonsai_mapping(ctx.clone(), nodes.clone())
                .map(|hg_bcs_mapping| {
                    let hg_bcs_mapping: HashMap<_, _> = hg_bcs_mapping.into_iter().collect();
                    nodes
                        .into_iter()
                        .map(move |node| hg_bcs_mapping.contains_key(&node))
                        .collect::<Vec<_>>()
                })
                .timeout(*TIMEOUT)
                .map_err(process_timeout_error)
                .traced(self.session.trace(), ops::KNOWNNODES, trace_args!())
                .timed(move |stats, known_nodes| {
                    if let Ok(known) = known_nodes {
                        ctx.perf_counters()
                            .add_to_counter(PerfCounterType::NumKnown, known.len() as i64);
                        ctx.perf_counters().add_to_counter(
                            PerfCounterType::NumUnknown,
                            (nodes_len - known.len()) as i64,
                        );
                    }
                    command_logger.without_wireproto().finalize_command(&stats);
                    Ok(())
                })
        })
    }

    // @wireprotocommand('getbundle', '*')
    fn getbundle(&self, args: GetbundleArgs) -> BoxStream<BytesOld, Error> {
        self.command_stream(ops::GETBUNDLE, |ctx, command_logger| {
            let value = json!({
                "bundlecaps": format_utf8_bytes_list(&args.bundlecaps),
                "common": debug_format_nodes(&args.common),
                "heads": debug_format_nodes(&args.heads),
                "listkeys": format_utf8_bytes_list(&args.listkeys),
            });
            let value = json!(vec![value]);

            let s = self
                .create_bundle(ctx.clone(), args)
                .whole_stream_timeout(*GETBUNDLE_TIMEOUT)
                .map_err(process_stream_timeout_error)
                .traced(self.session.trace(), ops::GETBUNDLE, trace_args!())
                .timed(move |stats, _| {
                    STATS::getbundle_ms
                        .add_value(stats.completion_time.as_millis_unchecked() as i64);
                    command_logger.finalize_command(ctx, &stats, Some(&value));
                    Ok(())
                })
                .boxify();

            throttle_stream(
                &self.session,
                Metric::EgressCommits,
                ops::GETBUNDLE,
                move || s,
            )
        })
    }

    // @wireprotocommand('hello')
    fn hello(&self) -> HgCommandRes<HashMap<String, Vec<String>>> {
        self.command_future(ops::HELLO, |_ctx, command_logger| {
            let mut res = HashMap::new();
            let mut caps = wireprotocaps();
            caps.push(format!("bundle2={}", bundle2caps()));
            res.insert("capabilities".to_string(), caps);

            future_old::ok(res)
                .timeout(*TIMEOUT)
                .map_err(process_timeout_error)
                .traced(self.session.trace(), ops::HELLO, trace_args!())
                .timed(move |stats, _| {
                    command_logger.without_wireproto().finalize_command(&stats);
                    Ok(())
                })
        })
    }

    // @wireprotocommand('listkeys', 'namespace')
    fn listkeys(&self, namespace: String) -> HgCommandRes<HashMap<Vec<u8>, Vec<u8>>> {
        if namespace == "bookmarks" {
            self.command_future(ops::LISTKEYS, |ctx, command_logger| {
                self.get_pull_default_bookmarks_maybe_stale(ctx.clone())
                    .traced(self.session.trace(), ops::LISTKEYS, trace_args!())
                    .timed(move |stats, _| {
                        command_logger.without_wireproto().finalize_command(&stats);
                        Ok(())
                    })
            })
        } else {
            info!(
                self.logging.logger(),
                "unsupported listkeys namespace: {}", namespace
            );
            future_old::ok(HashMap::new()).boxify()
        }
    }

    // @wireprotocommand('listkeyspatterns', 'namespace', 'patterns *')
    fn listkeyspatterns(
        &self,
        namespace: String,
        patterns: Vec<String>,
    ) -> HgCommandRes<BTreeMap<String, HgChangesetId>> {
        if namespace != "bookmarks" {
            info!(
                self.logging.logger(),
                "unsupported listkeyspatterns namespace: {}", namespace,
            );
            return future_old::err(format_err!(
                "unsupported listkeyspatterns namespace: {}",
                namespace
            ))
            .boxify();
        }

        self.command_future(ops::LISTKEYSPATTERNS, |ctx, command_logger| {
            let queries = patterns.into_iter().map({
                cloned!(ctx);
                let max = self.repo.list_keys_patterns_max();
                let repo = self.repo.blobrepo();
                move |pattern| {
                    if pattern.ends_with("*") {
                        // prefix match
                        let prefix =
                            try_boxfuture!(BookmarkPrefix::new(&pattern[..pattern.len() - 1]));
                        repo.get_bookmarks_by_prefix_maybe_stale(ctx.clone(), &prefix, max)
                            .map(|(bookmark, cs_id): (Bookmark, HgChangesetId)| {
                                (bookmark.into_name().to_string(), cs_id)
                            })
                            .collect()
                            .and_then(move |bookmarks| {
                                if bookmarks.len() < max as usize {
                                    Ok(bookmarks)
                                } else {
                                    Err(format_err!(
                                        "Bookmark query was truncated after {} results, use a more specific prefix search.",
                                        max,
                                    ))
                                }
                            })
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
                .timeout(*TIMEOUT)
                .map_err(process_timeout_error)
                .traced(self.session.trace(), ops::LISTKEYS, trace_args!())
                .timed(move |stats, _| {
                    command_logger.without_wireproto().finalize_command(&stats);
                    Ok(())
                })
        })
    }

    // @wireprotocommand('unbundle')
    fn unbundle(
        &self,
        _heads: Vec<String>,
        stream: BoxStream<Bundle2Item, Error>,
        maybe_full_content: Option<Arc<Mutex<BytesOld>>>,
    ) -> HgCommandRes<BytesOld> {
        let reponame = self.repo.reponame().clone();
        cloned!(
            self.session_bookmarks_cache,
            self as repoclient,
            self.repo as mononoke_repo
        );

        let hook_manager = self.repo.hook_manager();

        // Kill the saved set of bookmarks here - the unbundle may change them, and the next
        // command in sequence will need to fetch a new set
        self.session_bookmarks_cache.drop_cache();

        let lfs_params = self.lfs_params();

        self.repo
            .readonly()
            // Assume read only if we have an error.
            .or_else(|_| {
                ok(RepoReadOnly::ReadOnly(
                    "Failed to fetch repo lock status".to_string(),
                ))
            })
            .and_then(move |read_write| {
                let client = repoclient.clone();
                let trace = client.session.trace().clone();
                repoclient.command_future(ops::UNBUNDLE, move |ctx, command_logger| {
                    async move {
                        let blobrepo = client.repo.blobrepo();
                        let bookmark_attrs = client.repo.bookmark_attrs();
                        let lca_hint = client.repo.lca_hint().clone();
                        let infinitepush_params = client.repo.infinitepush().clone();
                        let infinitepush_writes_allowed = infinitepush_params.allow_writes;
                        let pushrebase_params = client.repo.pushrebase_params().clone();
                        let push_params = client.repo.push_params().clone();
                        let pure_push_allowed = push_params.pure_push_allowed;
                        let reponame = client.repo.reponame().clone();

                        let pushrebase_flags = pushrebase_params.flags.clone();
                        let res = unbundle::resolve(
                            &ctx,
                            &blobrepo,
                            infinitepush_writes_allowed,
                            stream,
                            read_write,
                            maybe_full_content,
                            pure_push_allowed,
                            pushrebase_flags,
                        )
                        .await;
                        match res {
                            Err(e) => Err(e.into()),
                            Ok((action, bypass_readonly)) => {
                                let unbundle_future = async {
                                    run_hooks(ctx.clone(), blobrepo.clone(), hook_manager, &action)
                                        .compat()
                                        .await?;

                                    let response = match client
                                        .maybe_get_pushredirector_for_action(&ctx, &action)?
                                    {
                                        Some(push_redirector) => {
                                            let ctx = ctx.with_mutated_scuba(|mut sample| {
                                                sample.add(
                                                    "target_repo_name",
                                                    push_redirector.repo.reponame().as_ref(),
                                                );
                                                sample.add(
                                                    "target_repo_id",
                                                    push_redirector.repo.repoid().id(),
                                                );
                                                sample
                                            });
                                            ctx.scuba().clone().log_with_msg(
                                                "Push redirected to large repo",
                                                None,
                                            );
                                            push_redirector
                                                .run_redirected_post_resolve_action(ctx, action)
                                                .await
                                        }
                                        None => {
                                            let maybe_reverse_filler_queue =
                                                client.repo.maybe_reverse_filler_queue();
                                            run_post_resolve_action(
                                                &ctx,
                                                &blobrepo,
                                                &bookmark_attrs,
                                                &*lca_hint,
                                                &infinitepush_params,
                                                &pushrebase_params,
                                                &push_params,
                                                maybe_reverse_filler_queue,
                                                action,
                                            )
                                            .await
                                        }
                                    };

                                    let response = response?
                                        .generate_bytes(
                                            ctx.clone(),
                                            blobrepo.clone(),
                                            reponame,
                                            pushrebase_params,
                                            lca_hint,
                                            lfs_params,
                                        )
                                        .map_err(Error::from)
                                        .compat()
                                        .await?;

                                    Ok(response)
                                };

                                let response = if bypass_readonly == true {
                                    unbundle_future.await
                                } else {
                                    let repo_lock = check_lock_repo(mononoke_repo);
                                    pin_mut!(repo_lock, unbundle_future);
                                    select(repo_lock, unbundle_future)
                                        .then(|either| async move {
                                            match either {
                                                Either::Left((repo_locked, _)) => repo_locked,
                                                Either::Right((unbundle, _)) => unbundle,
                                            }
                                        })
                                        .await
                                };
                                if response.is_ok() {
                                    // There's a bookmarks race condition where the client requests bookmarks after we return commits to it,
                                    // and is then confused because the bookmarks refer to commits that it doesn't know about. Ultimately,
                                    // this is something we need to resolve by sending down the commits we know the client doesn't have,
                                    // or by getting bookmarks atomically with the commits we send back.
                                    //
                                    // This tries to minimise the duration of the bookmarks race condition - we've just updated bookmarks,
                                    // and now we fill the cache with new bookmark data, so that, with luck, the bookmark update we see
                                    // will just be from this client's push, rather than from a later push that came in during the RTT
                                    // needed to get the `listkeys` request from the client.
                                    //
                                    // Ultimately, it would be better to not have the client `listkeys` after the push, but instead
                                    // depend on the reply part with a bookmark change in - T57874233
                                    session_bookmarks_cache
                                        .update_publishing_bookmarks_after_push(ctx.clone())
                                        .compat()
                                        .await?;
                                }
                                response
                            }
                        }
                    }
                    .boxed()
                    .compat()
                    .inspect_err({
                        cloned!(reponame);
                        move |err| {
                            use unbundle::BundleResolverError::*;
                            match err {
                                HookError(hooks) => {
                                    let failed_hooks: HashSet<String> = hooks
                                        .iter()
                                        .map(|fail| fail.get_hook_name().to_string())
                                        .collect();

                                    for failed_hook in failed_hooks {
                                        STATS::push_hook_failure
                                            .add_value(1, (reponame.clone(), failed_hook));
                                    }
                                }
                                PushrebaseConflicts(..) => {
                                    STATS::push_conflicts.add_value(1, (reponame,));
                                }
                                RateLimitExceeded { .. } => {
                                    STATS::rate_limits_exceeded.add_value(1, (reponame,));
                                }
                                Error(..) => {
                                    STATS::push_error.add_value(1, (reponame,));
                                }
                            };
                        }
                    })
                    .map(bytes_ext::copy_from_new)
                    .from_err()
                    .timeout(*TIMEOUT)
                    .map_err(process_timeout_error)
                    .inspect(move |_| STATS::push_success.add_value(1, (reponame,)))
                    .traced(&trace, ops::UNBUNDLE, trace_args!())
                    .timed(move |stats, _| {
                        command_logger.without_wireproto().finalize_command(&stats);
                        Ok(())
                    })
                })
            })
            .boxify()
    }

    // @wireprotocommand('gettreepack', 'rootdir mfnodes basemfnodes directories')
    fn gettreepack(&self, params: GettreepackArgs) -> BoxStream<BytesOld, Error> {
        self.command_stream(ops::GETTREEPACK, |ctx, mut command_logger| {
            let mut args = serde_json::Map::new();
            args.insert(
                "rootdir".to_string(),
                debug_format_path(&params.rootdir).into(),
            );
            args.insert(
                "mfnodes".to_string(),
                debug_format_manifests(&params.mfnodes).into(),
            );
            args.insert(
                "basemfnodes".to_string(),
                debug_format_manifests(&params.basemfnodes).into(),
            );
            args.insert(
                "directories".to_string(),
                debug_format_directories(&params.directories).into(),
            );
            if let Some(depth) = params.depth {
                args.insert("depth".to_string(), depth.to_string().into());
            }

            let args = json!(vec![args]);

            ctx.scuba()
                .clone()
                .add("gettreepack_mfnodes", params.mfnodes.len())
                .add("gettreepack_directories", params.directories.len())
                .log_with_msg("Gettreepack Params", None);

            let s = self
                .gettreepack_untimed(ctx.clone(), params)
                .whole_stream_timeout(*TIMEOUT)
                .map_err(process_stream_timeout_error)
                .traced(self.session.trace(), ops::GETTREEPACK, trace_args!())
                .inspect({
                    cloned!(ctx);
                    move |bytes| {
                        ctx.perf_counters().add_to_counter(
                            PerfCounterType::GettreepackResponseSize,
                            bytes.len() as i64,
                        );
                        STATS::total_tree_size.add_value(bytes.len() as i64);
                        if ctx.session().is_quicksand() {
                            STATS::quicksand_tree_size.add_value(bytes.len() as i64);
                        }
                    }
                })
                .timed({
                    move |stats, _| {
                        if stats.completion_time > *SLOW_REQUEST_THRESHOLD {
                            command_logger.add_trimmed_scuba_extra("command_args", &args);
                        }
                        STATS::gettreepack_ms
                            .add_value(stats.completion_time.as_millis_unchecked() as i64);
                        command_logger.finalize_command(ctx, &stats, Some(&args));
                        Ok(())
                    }
                });

            throttle_stream(
                &self.session,
                Metric::EgressTotalManifests,
                ops::GETTREEPACK,
                move || s,
            )
        })
    }

    // @wireprotocommand('stream_out_shallow')
    fn stream_out_shallow(&self) -> BoxStream<BytesOld, Error> {
        self.command_stream(ops::STREAMOUTSHALLOW, |ctx, command_logger| {
            let changelog = match self.repo.streaming_clone() {
                None => Ok(RevlogStreamingChunks::new()).into_future().left_future(),
                Some(SqlStreamingCloneConfig {
                    blobstore,
                    fetcher,
                    repoid,
                }) => fetcher
                    .fetch_changelog(ctx.clone(), *repoid, blobstore.clone())
                    .right_future(),
            };

            changelog
                .map({
                    cloned!(ctx);
                    move |chunk| {
                        let data_blobs = chunk
                            .data_blobs
                            .into_iter()
                            .map(|fut| {
                                fut.timed({
                                    let ctx = ctx.clone();
                                    move |stats, blob| {
                                        ctx.perf_counters().add_to_counter(
                                            PerfCounterType::SumManifoldPollTime,
                                            stats.poll_time.as_nanos_unchecked() as i64,
                                        );
                                        if let Ok(bytes) = blob {
                                            ctx.perf_counters().add_to_counter(
                                                PerfCounterType::BytesSent,
                                                bytes.len() as i64,
                                            )
                                        }
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
                                    move |stats, blob| {
                                        ctx.perf_counters().add_to_counter(
                                            PerfCounterType::SumManifoldPollTime,
                                            stats.poll_time.as_nanos_unchecked() as i64,
                                        );
                                        if let Ok(bytes) = blob {
                                            ctx.perf_counters().add_to_counter(
                                                PerfCounterType::BytesSent,
                                                bytes.len() as i64,
                                            )
                                        }
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
                    cloned!(ctx);
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
                        ) -> impl Stream<Item = Bytes, Error = Error> + Send
                        {
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
                .whole_stream_timeout(*CLONE_TIMEOUT)
                .map(bytes_ext::copy_from_new)
                .map_err(process_stream_timeout_error)
                .timed({
                    move |stats, _| {
                        command_logger.finalize_command(ctx, &stats, None);
                        Ok(())
                    }
                })
        })
    }

    // @wireprotocommand('getpackv1')
    fn getpackv1(
        &self,
        params: BoxStream<(MPath, Vec<HgFileNodeId>), Error>,
    ) -> BoxStream<BytesOld, Error> {
        self.getpack(
            params,
            |ctx, repo, node, _lfs_thresold, validate_hash| {
                create_getpack_v1_blob(ctx, repo, node, validate_hash).map(|(size, fut)| {
                    // GetpackV1 has no metadata.
                    let fut = fut.map(|(id, bytes)| (id, bytes, None));
                    (size, fut)
                })
            },
            ops::GETPACKV1,
        )
    }

    // @wireprotocommand('getpackv2')
    fn getpackv2(
        &self,
        params: BoxStream<(MPath, Vec<HgFileNodeId>), Error>,
    ) -> BoxStream<BytesOld, Error> {
        self.getpack(
            params,
            |ctx, repo, node, lfs_thresold, validate_hash| {
                create_getpack_v2_blob(ctx, repo, node, lfs_thresold, validate_hash).map(
                    |(size, fut)| {
                        // GetpackV2 always has metadata.
                        let fut = fut.map(|(id, bytes, metadata)| (id, bytes, Some(metadata)));
                        (size, fut)
                    },
                )
            },
            ops::GETPACKV2,
        )
    }

    // @wireprotocommand('getcommitdata', 'nodes *'), but the * is ignored
    fn getcommitdata(&self, nodes: Vec<HgChangesetId>) -> BoxStream<BytesOld, Error> {
        self.command_stream(ops::GETCOMMITDATA, |ctx, mut command_logger| {
            let args = json!(nodes);
            let blobrepo = self.repo.blobrepo().clone();
            ctx.scuba()
                .clone()
                .add("getcommitdata_nodes", nodes.len())
                .log_with_msg("GetCommitData Params", None);
            let s = stream::iter_ok::<_, Error>(nodes.into_iter())
                .map({
                    cloned!(ctx);
                    move |hg_cs_id| {
                        RevlogChangeset::load(ctx.clone(), blobrepo.blobstore(), hg_cs_id)
                            .and_then(move |revlog_cs| serialize_getcommitdata(hg_cs_id, revlog_cs))
                    }
                })
                .buffered(100)
                .whole_stream_timeout(*TIMEOUT)
                .map_err(process_stream_timeout_error)
                .inspect({
                    cloned!(ctx);
                    move |bytes| {
                        ctx.perf_counters().add_to_counter(
                            PerfCounterType::GetcommitdataResponseSize,
                            bytes.len() as i64,
                        );
                        ctx.perf_counters()
                            .increment_counter(PerfCounterType::GetcommitdataNumCommits);
                        STATS::getcommitdata_commit_count.add_value(1);
                    }
                })
                .timed({
                    move |stats, _| {
                        if stats.completion_time > *SLOW_REQUEST_THRESHOLD {
                            command_logger.add_trimmed_scuba_extra("command_args", &args);
                        }
                        STATS::getcommitdata_ms
                            .add_value(stats.completion_time.as_millis_unchecked() as i64);
                        command_logger.finalize_command(ctx, &stats, Some(&args));
                        Ok(())
                    }
                });

            throttle_stream(
                &self.session,
                Metric::EgressCommits,
                ops::GETCOMMITDATA,
                move || s,
            )
        })
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
) -> BoxStream<(HgManifestId, Option<MPath>), Error> {
    let GettreepackArgs {
        rootdir,
        mfnodes,
        basemfnodes,
        depth: fetchdepth,
        directories,
    } = params;

    if fetchdepth == Some(1) && directories.len() > 0 {
        if directories.len() != mfnodes.len() {
            let e = format_err!(
                "invalid directories count ({}, expected {})",
                directories.len(),
                mfnodes.len()
            );
            return stream::once(Err(e)).boxify();
        }

        if rootdir.is_some() {
            let e = Error::msg("rootdir must be empty");
            return stream::once(Err(e)).boxify();
        }

        if !basemfnodes.is_empty() {
            let e = Error::msg("basemfnodes must be empty");
            return stream::once(Err(e)).boxify();
        }

        let entries = mfnodes
            .into_iter()
            .zip(directories.into_iter())
            .map(|(node, path)| {
                let path = if path.len() > 0 {
                    Some(MPath::new(path.as_ref())?)
                } else {
                    None
                };
                Ok((node, path))
            })
            .collect::<Result<Vec<_>, Error>>();

        let entries = try_boxstream!(entries);

        ctx.perf_counters().set_counter(
            PerfCounterType::GettreepackDesignatedNodes,
            entries.len() as i64,
        );

        return stream::iter_ok::<_, Error>(entries).boxify();
    }

    if !directories.is_empty() {
        // This param is not used by core hg, don't worry about implementing it now
        return stream::once(Err(Error::msg("directories param is not supported"))).boxify();
    }

    // 65536 matches the default TREE_DEPTH_MAX value from Mercurial
    let fetchdepth = fetchdepth.unwrap_or(2 << 16);

    // TODO(stash): T25850889 only one basemfnodes is used. That means that trees that client
    // already has can be sent to the client.
    let mut basemfnode = basemfnodes.iter().next().cloned();

    cloned!(repo);
    stream::iter_ok::<_, Error>(
        mfnodes
            .into_iter()
            .filter(move |node| !basemfnodes.contains(&node))
            .map(move |mfnode| {
                let cur_basemfnode = basemfnode.unwrap_or(HgManifestId::new(NULL_HASH));
                // `basemfnode`s are used to reduce the data we send the client by having us prune
                // manifests the client already has. If the client claims to have no manifests,
                // then give it a full set for the first manifest it requested, then give it diffs
                // against the manifest we now know it has (the one we're sending), to reduce
                // the data we send.
                if basemfnode.is_none() {
                    basemfnode = Some(mfnode);
                }

                get_changed_manifests_stream(
                    ctx.clone(),
                    &repo,
                    mfnode,
                    cur_basemfnode,
                    rootdir.clone(),
                    fetchdepth,
                )
            }),
    )
    .flatten()
    .boxify()
}

fn get_changed_manifests_stream(
    ctx: CoreContext,
    repo: &BlobRepo,
    mfid: HgManifestId,
    basemfid: HgManifestId,
    rootpath: Option<MPath>,
    max_depth: usize,
) -> BoxStream<(HgManifestId, Option<MPath>), Error> {
    if max_depth == 1 {
        return stream::iter_ok(vec![(mfid, rootpath)]).boxify();
    }

    basemfid
        .filtered_diff(
            ctx,
            repo.get_blobstore(),
            mfid,
            |output_diff| {
                let (path, entry) = match output_diff {
                    Diff::Added(path, entry) | Diff::Changed(path, _, entry) => (path, entry),
                    Diff::Removed(..) => {
                        return None;
                    }
                };
                match entry {
                    Entry::Tree(hg_mf_id) => Some((path, hg_mf_id)),
                    Entry::Leaf(_) => None,
                }
            },
            move |tree_diff| match tree_diff {
                Diff::Added(path, ..) | Diff::Changed(path, ..) => match path {
                    Some(path) => path.num_components() <= max_depth,
                    None => true,
                },
                Diff::Removed(..) => false,
            },
        )
        .map(move |(path_no_root_path, hg_mf_id)| {
            let mut path = rootpath.clone();
            path.extend(MPath::into_iter_opt(path_no_root_path));
            (hg_mf_id, path)
        })
        .boxify()
}

pub fn fetch_treepack_part_input(
    ctx: CoreContext,
    repo: &BlobRepo,
    hg_mf_id: HgManifestId,
    path: Option<MPath>,
    validate_content: bool,
) -> BoxFuture<parts::TreepackPartInput, Error> {
    let repo_path = match path {
        Some(path) => RepoPath::DirectoryPath(path),
        None => RepoPath::RootPath,
    };

    let envelope_fut =
        fetch_manifest_envelope(ctx.clone(), &repo.get_blobstore().boxed(), hg_mf_id);

    let filenode_fut = repo
        .get_filenode_opt(
            ctx.clone(),
            &repo_path,
            HgFileNodeId::new(hg_mf_id.into_nodehash()),
        )
        .map(|filenode_res| {
            match filenode_res {
                FilenodeResult::Present(maybe_filenode) => maybe_filenode,
                // Filenodes are disabled - that means we can't fetch
                // linknode so we'll return NULL to clients.
                FilenodeResult::Disabled => None,
            }
        });

    filenode_fut
        .join(envelope_fut)
        .map({
            cloned!(ctx);
            move |(maybe_filenode, envelope)| {
                let content = envelope.contents().clone();
                match maybe_filenode {
                    Some(filenode) => {
                        let p1 = filenode.p1.map(|p| p.into_nodehash());
                        let p2 = filenode.p2.map(|p| p.into_nodehash());
                        let parents = HgParents::new(p1, p2);
                        let linknode = filenode.linknode;
                        (parents, linknode, content)
                    }
                    // Filenodes might not be present. For example we don't have filenodes for
                    // infinitepush commits. In that case fetch parents from manifest, but we can't
                    // fetch the linknode, so set it to NULL_CSID. Client can handle null linknode,
                    // though it can cause slowness sometimes.
                    None => {
                        ctx.perf_counters()
                            .increment_counter(PerfCounterType::NullLinknode);
                        STATS::null_linknode_gettreepack.add_value(1);
                        let (p1, p2) = envelope.parents();
                        let parents = HgParents::new(p1, p2);

                        (parents, NULL_CSID, content)
                    }
                }
            }
        })
        .and_then(move |(parents, linknode, content)| {
            if validate_content {
                validate_manifest_content(
                    ctx,
                    hg_mf_id.into_nodehash(),
                    &content,
                    &repo_path,
                    &parents,
                )?;
            }

            let fullpath = repo_path.into_mpath();
            let (p1, p2) = parents.get_nodes();
            Ok(parts::TreepackPartInput {
                node: hg_mf_id.into_nodehash(),
                p1,
                p2,
                content: bytes_ext::copy_from_new(content),
                fullpath,
                linknode: linknode.into_nodehash(),
            })
        })
        .boxify()
}

fn validate_manifest_content(
    ctx: CoreContext,
    actual: HgNodeHash,
    content: &[u8],
    path: &RepoPath,
    parents: &HgParents,
) -> Result<(), Error> {
    let expected = calculate_hg_node_id(&content, &parents);

    // Do not do verification for a root node because it might be broken
    // because of migration to tree manifest.
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
            path: path.clone(),
            expected,
            actual,
        }
        .into())
    }
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

fn serialize_getcommitdata(
    hg_cs_id: HgChangesetId,
    revlog_changeset: Option<RevlogChangeset>,
) -> Result<BytesOld> {
    // For each changeset, write:
    //
    //   HEX(HASH) + ' ' + STR(LEN(SERIALIZED)) + '\n' + SERIALIZED + '\n'
    //
    // For known changesets, SERIALIZED is the payload that SHA1(SERIALIZED)
    // matches HASH. The client relies on this for data integrity check.
    //
    // For unknown and NULL changesets, SERIALIZED is empty and the client
    // should check that to know that commits are missing on the server.
    let mut revlog_commit = Vec::new();
    if hg_cs_id != NULL_CSID {
        if let Some(real_changeset) = revlog_changeset {
            real_changeset.generate_for_hash_verification(&mut revlog_commit)?;
        }
    }
    // capacity = hash + " " + length + "\n" + content + "\n"
    let mut buffer = BytesMutOld::with_capacity(40 + 1 + 10 + 1 + revlog_commit.len() + 1);
    write!(buffer, "{} {}\n", hg_cs_id, revlog_commit.len())?;
    buffer.extend_from_slice(&revlog_commit);
    buffer.put("\n");
    Ok(buffer.freeze())
}

fn with_command_monitor<T>(ctx: CoreContext, t: T) -> Monitor<T, Sender<()>> {
    let (sender, receiver) = oneshot::channel();

    let reporting_loop = async move {
        let start = Instant::now();

        loop {
            let interval = match tunables().get_command_monitor_interval().try_into() {
                Ok(interval) if interval > 0 => interval,
                _ => {
                    break;
                }
            };

            tokio::time::delay_for(Duration::from_secs(interval)).await;

            if tunables().get_command_monitor_remote_logging() != 0 {
                info!(
                    ctx.logger(),
                    "Command in progress. Elapsed: {}s, BlobPuts: {}, BlobGets: {}, SqlWrites: {}, SqlReadsMaster: {}, SqlReadsReplica: {}.",
                    start.elapsed().as_secs(),
                    ctx.perf_counters().get_counter(PerfCounterType::BlobPuts),
                    ctx.perf_counters().get_counter(PerfCounterType::BlobGets),
                    ctx.perf_counters().get_counter(PerfCounterType::SqlWrites),
                    ctx.perf_counters().get_counter(PerfCounterType::SqlReadsMaster),
                    ctx.perf_counters().get_counter(PerfCounterType::SqlReadsReplica),
                    ; o!("remote" => "true")
                );
            }

            let mut scuba = ctx.scuba().clone();
            ctx.perf_counters().insert_perf_counters(&mut scuba);
            scuba.log_with_msg("Long running command", None);
        }
    };

    tokio::task::spawn(async move {
        futures::pin_mut!(reporting_loop);
        let _ = future::select(reporting_loop, receiver).await;
    });

    Monitor::new(t, sender)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_format_directories() {
        assert_eq!(&debug_format_directories(vec![&"foo"]), "foo,");
        assert_eq!(&debug_format_directories(vec![&"foo,bar"]), "foo:obar,");
        assert_eq!(&debug_format_directories(vec![&"foo", &"bar"]), "foo,bar,");
    }
}
