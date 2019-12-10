/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::errors::ErrorKind;

use unbundle::{run_hooks, run_post_resolve_action, PushRedirector};

use anyhow::{format_err, Error, Result};
use blobrepo::BlobRepo;
use bookmarks::{Bookmark, BookmarkName, BookmarkPrefix};
use bytes::{BufMut, Bytes, BytesMut};
use cloned::cloned;
use configerator::ConfigLoader;
use context::{CoreContext, LoggingContainer, Metric, PerfCounterType, SessionContainer};
use fbwhoami::FbWhoAmI;
use futures::future::ok;
use futures::{future, stream, try_ready, Async, Future, IntoFuture, Poll, Stream};
use futures_ext::{
    select_all, try_boxfuture, try_boxstream, BoxFuture, BoxStream, BufferedParams, FutureExt,
    StreamExt, StreamTimeoutError,
};
use futures_stats::{Timed, TimedStreamTrait};
use getbundle_response::create_getbundle_response;
use hgproto::{GetbundleArgs, GettreepackArgs, HgCommandRes, HgCommands};
use hooks::{HookExecution, HookManager};
use itertools::Itertools;
use lazy_static::lazy_static;
use manifest::{Diff, Entry, ManifestOps};
use maplit::hashmap;
use mercurial_bundles::{create_bundle_stream, parts, wirepack, Bundle2Item};
use mercurial_types::{
    blobs::HgBlobChangeset, calculate_hg_node_id, convert_parents_to_remotefilelog_format,
    fetch_manifest_envelope, percent_encode, Delta, HgChangesetId, HgFileNodeId, HgManifestId,
    HgNodeHash, HgParents, MPath, RepoPath, NULL_CSID, NULL_HASH,
};
use metaconfig_types::RepoReadOnly;
use mononoke_repo::{MononokeRepo, SqlStreamingCloneConfig};
use mononoke_types::RepositoryId;
use pushredirect_enable::types::MononokePushRedirectEnable;
use rand::{self, Rng};
use remotefilelog::{
    create_getfiles_blob, create_getpack_v1_blob, create_getpack_v2_blob,
    get_unordered_file_history_for_multiple_nodes,
};
use revisionstore::Metadata;
use scuba_ext::ScubaSampleBuilderExt;
use serde_json::{self, json};
use slog::{debug, info, o};
use stats::{define_stats, DynamicTimeseries, Histogram, Timeseries};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::mem;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use streaming_clone::RevlogStreamingChunks;
use time_ext::DurationExt;
use tokio::timer::timeout::Error as TimeoutError;
use tokio::util::FutureExt as TokioFutureExt;
use tracing::{trace_args, Traced};

mod logging;

use logging::CommandLogger;
pub use logging::WireprotoLogging;

const CONFIGERATOR_TIMEOUT: Duration = Duration::from_millis(25);

define_stats! {
    prefix = "mononoke.repo_client";
    getbundle_ms:
        histogram(10, 0, 1_000, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    gettreepack_ms:
        histogram(2, 0, 200, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    getfiles_ms:
        histogram(5, 0, 500, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    getpack_ms:
        histogram(20, 0, 2_000, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    total_tree_count: timeseries(RATE, SUM),
    quicksand_tree_count: timeseries(RATE, SUM),
    total_tree_size: timeseries(RATE, SUM),
    quicksand_tree_size: timeseries(RATE, SUM),
    total_fetched_file_size: timeseries(RATE, SUM),
    quicksand_fetched_file_size: timeseries(RATE, SUM),

    push_success: dynamic_timeseries("push_success.{}", (reponame: String); RATE, SUM),
    push_hook_failure: dynamic_timeseries("push_hook_failure.{}.{}", (reponame: String, hook_failure: String); RATE, SUM),
    push_conflicts: dynamic_timeseries("push_conflicts.{}", (reponame: String); RATE, SUM),
    rate_limits_exceeded: dynamic_timeseries("rate_limits_exceeded.{}", (reponame: String); RATE, SUM),
    push_error: dynamic_timeseries("push_error.{}", (reponame: String); RATE, SUM),
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

fn format_nodes<'a>(nodes: impl IntoIterator<Item = &'a HgChangesetId>) -> String {
    nodes.into_iter().map(|node| format!("{}", node)).join(" ")
}

fn format_manifests<'a>(nodes: impl IntoIterator<Item = &'a HgManifestId>) -> String {
    nodes.into_iter().map(|node| format!("{}", node)).join(" ")
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
    static ref CLONE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
    // getfiles requests can be rather long. Let's bump the timeout
    static ref GETFILES_TIMEOUT: Duration = Duration::from_secs(90 * 60);
    static ref LOAD_LIMIT_TIMEFRAME: Duration = Duration::from_secs(1);
    static ref SLOW_REQUEST_THRESHOLD: Duration = Duration::from_secs(1);
}

fn process_timeout_error(err: TimeoutError<Error>) -> Error {
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
    ]
}

fn bundle2caps(support_bundle2_listkeys: bool) -> String {
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
    wireproto_logging: Arc<WireprotoLogging>,
    maybe_push_redirector: Option<PushRedirector>,
    pushredirect_config: Option<ConfigLoader>,
}

fn get_pull_default_bookmarks_maybe_stale_raw(
    ctx: CoreContext,
    repo: BlobRepo,
) -> impl Future<Item = HashMap<Vec<u8>, Vec<u8>>, Error = Error> {
    repo.get_pull_default_bookmarks_maybe_stale(ctx)
        .map(|(book, cs): (Bookmark, HgChangesetId)| {
            let hash: Vec<u8> = cs.into_nodehash().to_hex().into();
            (book.into_name().into_byte_vec(), hash)
        })
        .fold(HashMap::new(), |mut map, item| {
            map.insert(item.0, item.1);
            let ret: Result<_, Error> = Ok(map);
            ret
        })
        .timeout(*BOOKMARKS_TIMEOUT)
        .map_err(process_timeout_error)
}

fn update_pull_default_bookmarks_maybe_stale_cache_raw(
    cache: Arc<Mutex<Option<HashMap<Vec<u8>, Vec<u8>>>>>,
    bookmarks: HashMap<Vec<u8>, Vec<u8>>,
) {
    let mut maybe_cache = cache.lock().expect("lock poisoned");
    *maybe_cache = Some(bookmarks);
}

fn update_pull_default_bookmarks_maybe_stale_cache(
    ctx: CoreContext,
    cache: Arc<Mutex<Option<HashMap<Vec<u8>, Vec<u8>>>>>,
    repo: BlobRepo,
) -> impl Future<Item = (), Error = Error> {
    get_pull_default_bookmarks_maybe_stale_raw(ctx, repo)
        .map(move |bookmarks| update_pull_default_bookmarks_maybe_stale_cache_raw(cache, bookmarks))
}

fn get_pull_default_bookmarks_maybe_stale_updating_cache(
    ctx: CoreContext,
    cache: Arc<Mutex<Option<HashMap<Vec<u8>, Vec<u8>>>>>,
    repo: BlobRepo,
    update_cache: bool,
) -> impl Future<Item = HashMap<Vec<u8>, Vec<u8>>, Error = Error> {
    if update_cache {
        get_pull_default_bookmarks_maybe_stale_raw(ctx, repo)
            .inspect(move |bookmarks| {
                update_pull_default_bookmarks_maybe_stale_cache_raw(cache, bookmarks.clone())
            })
            .left_future()
    } else {
        get_pull_default_bookmarks_maybe_stale_raw(ctx, repo).right_future()
    }
}

impl RepoClient {
    pub fn new(
        repo: MononokeRepo,
        session: SessionContainer,
        logging: LoggingContainer,
        hash_validation_percentage: usize,
        preserve_raw_bundle2: bool,
        pure_push_allowed: bool,
        hook_manager: Arc<HookManager>,
        support_bundle2_listkeys: bool,
        wireproto_logging: Arc<WireprotoLogging>,
        maybe_push_redirector: Option<PushRedirector>,
        pushredirect_config: Option<ConfigLoader>,
    ) -> Self {
        RepoClient {
            repo,
            session,
            logging,
            hash_validation_percentage,
            preserve_raw_bundle2,
            pure_push_allowed,
            hook_manager,
            cached_pull_default_bookmarks_maybe_stale: Arc::new(Mutex::new(None)),
            support_bundle2_listkeys,
            wireproto_logging,
            maybe_push_redirector,
            pushredirect_config,
        }
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

        let ctx = self.session.new_context(logger, scuba);

        let command_logger = CommandLogger::new(
            ctx.clone(),
            command.to_owned(),
            self.wireproto_logging.clone(),
        );

        (ctx, command_logger)
    }

    fn get_pull_default_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = HashMap<Vec<u8>, Vec<u8>>, Error = Error> {
        let maybe_cache = self
            .cached_pull_default_bookmarks_maybe_stale
            .lock()
            .expect("lock poisoned")
            .clone();

        match maybe_cache {
            None => get_pull_default_bookmarks_maybe_stale_updating_cache(
                ctx,
                self.cached_pull_default_bookmarks_maybe_stale.clone(),
                self.repo.blobrepo().clone(),
                self.support_bundle2_listkeys,
            )
            .left_future(),
            Some(bookmarks) => future::ok(bookmarks).right_future(),
        }
    }

    fn create_bundle(
        &self,
        ctx: CoreContext,
        args: GetbundleArgs,
    ) -> Result<BoxStream<Bytes, Error>> {
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

        bundle2_parts.append(&mut create_getbundle_response(
            ctx.clone(),
            blobrepo.clone(),
            args.common,
            args.heads,
            self.repo.lca_hint().clone(),
            if use_phases {
                Some(self.repo.phases_hint().clone())
            } else {
                None
            },
        )?);

        // listkeys bookmarks part is added separately.

        // TODO: generalize this to other listkey types
        // (note: just calling &b"bookmarks"[..] doesn't work because https://fburl.com/0p0sq6kp)
        if args.listkeys.contains(&b"bookmarks".to_vec()) {
            let items = self
                .get_pull_default_bookmarks_maybe_stale(ctx)
                .map(|bookmarks| stream::iter_ok(bookmarks))
                .flatten_stream();
            bundle2_parts.push(parts::listkey_part("bookmarks", items)?);
        }
        // TODO(stash): handle includepattern= and excludepattern=

        let compression = None;
        Ok(create_bundle_stream(bundle2_parts, compression).boxify())
    }

    fn gettreepack_untimed(
        &self,
        ctx: CoreContext,
        params: GettreepackArgs,
    ) -> BoxStream<Bytes, Error> {
        let validate_hash = rand::random::<usize>() % 100 < self.hash_validation_percentage;
        let changed_entries = gettreepack_entries(ctx.clone(), self.repo.blobrepo(), params)
            .filter({
                let mut used_hashes = HashSet::new();
                move |(hg_mf_id, _)| used_hashes.insert(hg_mf_id.clone())
            })
            .map({
                cloned!(ctx);
                let blobrepo = self.repo.blobrepo().clone();
                move |(hg_mf_id, path)| {
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
    ) -> BoxStream<Bytes, Error>
    where
        WeightedContent: Future<Item = (u64, Content), Error = Error> + Send + 'static,
        Content:
            Future<Item = (HgFileNodeId, Bytes, Option<Metadata>), Error = Error> + Send + 'static,
        GetpackHandler:
            Fn(CoreContext, BlobRepo, HgFileNodeId, Option<u64>, bool) -> WeightedContent
                + Send
                + 'static,
    {
        let (ctx, command_logger) = self.start_command(name);

        // We buffer all parameters in memory so that we can log them.
        // That shouldn't be a problem because requests are quite small
        let getpack_params = Arc::new(Mutex::new(vec![]));
        let repo = self.repo.blobrepo().clone();

        let lfs_threshold = self.repo.lfs_params().threshold;

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
                .map(|v| stream::iter_ok(v.into_iter()))
                .flatten_stream()
                .map({
                    cloned!(ctx, getpack_params, repo);
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
                                    lfs_threshold,
                                    validate_hash,
                                )
                            })
                            .collect();

                        let history_fut = get_unordered_file_history_for_multiple_nodes(
                            ctx.clone(),
                            repo.clone(),
                            filenodes.into_iter().collect(),
                            &path,
                        )
                        .collect();

                        future::join_all(blob_futs.into_iter()).map(move |blobs| {
                            let total_weight = blobs.iter().map(|(size, _)| size).sum();
                            let content_futs = blobs.into_iter().map(|(_, fut)| fut);
                            let contents_and_history = future::join_all(content_futs)
                                .join(history_fut)
                                .map(move |(contents, history)| (path, contents, history));

                            (contents_and_history, total_weight)
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
                .whole_stream_timeout(*GETFILES_TIMEOUT)
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
                        for (filenode, content, metadata) in contents {
                            let content = content.to_vec();
                            ctx.perf_counters().set_max_counter(
                                PerfCounterType::GetpackMaxFileSize,
                                content.len() as i64,
                            );
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

                        command_logger.finalize_command(ctx, &stats, Some(&json! {encoded_params}));

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
    }

    /// Returns Some(push_redirector) if pushredirect should redirect this push
    /// via the repo sync target, None if this should be a direct push
    fn maybe_get_push_redirector_for_action(
        &self,
        action: &unbundle::PostResolveAction,
    ) -> Result<Option<PushRedirector>> {
        // Don't query configerator if we lack config
        if self.maybe_push_redirector.is_none() {
            return Ok(None);
        }
        if maybe_pushredirect_action(
            self.repo.blobrepo().get_repoid(),
            self.pushredirect_config.as_ref(),
            action,
        )? {
            Ok(self.maybe_push_redirector.clone())
        } else {
            Ok(None)
        }
    }
}

fn maybe_pushredirect_action(
    repo_id: RepositoryId,
    pushredirect_config: Option<&ConfigLoader>,
    action: &unbundle::PostResolveAction,
) -> Result<bool> {
    let maybe_config: Option<MononokePushRedirectEnable> = match pushredirect_config {
        // If you chose not to give us configerator, we won't allow redirect based purely on config
        None => return Ok(false),
        Some(ref pushredirect_config) => {
            let data = pushredirect_config.load(CONFIGERATOR_TIMEOUT)?;
            serde_json::from_str(&data.contents)?
        }
    };

    let enabled = maybe_config.and_then(move |config| {
        config.per_repo.get(&(repo_id.id() as i64)).map(|enables| {
            use unbundle::PostResolveAction::*;

            match action {
                InfinitePush(_) => enables.draft_push,
                Push(_) | PushRebase(_) | BookmarkOnlyPushRebase(_) => enables.public_push,
            }
        })
    });
    Ok(enabled.unwrap_or(false))
}

fn throttle_stream<F, S, V>(
    session: &SessionContainer,
    metric: Metric,
    name: &'static str,
    func: F,
) -> BoxStream<V, Error>
where
    F: FnOnce() -> S + Send + 'static,
    S: Stream<Item = V, Error = Error> + Send + 'static,
{
    session
        .should_throttle(metric, *LOAD_LIMIT_TIMEFRAME)
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
        .boxify()
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

        let (ctx, command_logger) = self.start_command(ops::BETWEEN);

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
            .boxify()
    }

    // @wireprotocommand('clienttelemetry')
    fn clienttelemetry(&self, args: HashMap<Vec<u8>, Vec<u8>>) -> HgCommandRes<String> {
        let fallback_hostname = "<no hostname found>";
        let hostname = match FbWhoAmI::new() {
            Ok(fbwhoami) => fbwhoami.get_name().unwrap_or(fallback_hostname).to_string(),
            Err(_) => fallback_hostname.to_string(),
        };

        let (_ctx, mut command_logger) = self.start_command(ops::CLIENTTELEMETRY);

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

        future::ok(hostname)
            .timeout(*TIMEOUT)
            .map_err(process_timeout_error)
            .traced(self.session.trace(), ops::CLIENTTELEMETRY, trace_args!())
            .timed(move |stats, _| {
                command_logger.without_wireproto().finalize_command(&stats);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('heads')
    fn heads(&self) -> HgCommandRes<HashSet<HgChangesetId>> {
        // Get a stream of heads and collect them into a HashSet
        // TODO: directly return stream of heads
        let (ctx, command_logger) = self.start_command(ops::HEADS);

        // We get all bookmarks while handling heads to fix the race demonstrated in
        // test-bookmark-race.t - this fixes bookmarks at the moment the client starts discovery
        // NB: Getting bookmarks is only done here to ensure that they are cached at the beginning
        // of discovery - this function is meant to get heads only.
        self.get_pull_default_bookmarks_maybe_stale(ctx.clone())
            .join(
                self.repo
                    .blobrepo()
                    .get_heads_maybe_stale(ctx.clone())
                    .collect()
                    .map(|v| v.into_iter().collect())
                    .from_err(),
            )
            .map(|(_, r)| r)
            .timeout(*TIMEOUT)
            .map_err(process_timeout_error)
            .traced(self.session.trace(), ops::HEADS, trace_args!())
            .timed(move |stats, _| {
                command_logger.without_wireproto().finalize_command(&stats);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('lookup', 'key')
    fn lookup(&self, key: String) -> HgCommandRes<Bytes> {
        let (ctx, command_logger) = self.start_command(ops::LOOKUP);
        // TODO(stash): T25928839 lookup should support prefixes
        let repo = self.repo.blobrepo().clone();

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
                repo.changeset_exists(ctx.clone(), csid)
                    .and_then({
                        cloned!(ctx);
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
            (None, Some(bookmark)) => check_bookmark_exists(ctx.clone(), repo, bookmark),
            // Failed to parse as a hash or bookmark.
            _ => Ok(generate_resp_buf(false, "invalid input".as_bytes()))
                .into_future()
                .boxify(),
        };

        lookup_fut
            .timeout(*TIMEOUT)
            .map_err(process_timeout_error)
            .traced(self.session.trace(), ops::LOOKUP, trace_args!())
            .timed(move |stats, _| {
                command_logger.without_wireproto().finalize_command(&stats);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('known', 'nodes *'), but the '*' is ignored
    fn known(&self, nodes: Vec<HgChangesetId>) -> HgCommandRes<Vec<bool>> {
        let (ctx, mut command_logger) = self.start_command(ops::KNOWN);

        let blobrepo = self.repo.blobrepo().clone();

        let nodes_len = nodes.len();

        let phases_hint = self.repo.phases_hint().clone();

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
                        .get_public(ctx, blobrepo, bcs_ids)
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
                    let extra_context = json!({
                        "num_known": known.len(),
                        "num_unknown": nodes_len - known.len(),
                    })
                    .to_string();
                    command_logger.add_scuba_extra("extra_context", extra_context);
                }
                command_logger.without_wireproto().finalize_command(&stats);
                Ok(())
            })
            .boxify()
    }

    fn knownnodes(&self, nodes: Vec<HgChangesetId>) -> HgCommandRes<Vec<bool>> {
        let (ctx, mut command_logger) = self.start_command(ops::KNOWNNODES);

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
                    let extra_context = json!({
                        "num_known": known.len(),
                        "num_unknown": nodes_len - known.len(),
                    })
                    .to_string();
                    command_logger.add_scuba_extra("extra_context", extra_context);
                }
                command_logger.without_wireproto().finalize_command(&stats);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('getbundle', '*')
    fn getbundle(&self, args: GetbundleArgs) -> BoxStream<Bytes, Error> {
        let (ctx, command_logger) = self.start_command(ops::GETBUNDLE);

        let value = json!({
            "bundlecaps": format_utf8_bytes_list(&args.bundlecaps),
            "common": format_nodes(&args.common),
            "heads": format_nodes(&args.heads),
            "listkeys": format_utf8_bytes_list(&args.listkeys),
        });
        let value = json!(vec![value]);

        let s = match self.create_bundle(ctx.clone(), args) {
            Ok(res) => res.boxify(),
            Err(err) => stream::once(Err(err)).boxify(),
        }
        .whole_stream_timeout(*GETBUNDLE_TIMEOUT)
        .map_err(process_stream_timeout_error)
        .traced(self.session.trace(), ops::GETBUNDLE, trace_args!())
        .timed(move |stats, _| {
            STATS::getbundle_ms.add_value(stats.completion_time.as_millis_unchecked() as i64);
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
    }

    // @wireprotocommand('hello')
    fn hello(&self) -> HgCommandRes<HashMap<String, Vec<String>>> {
        let (_ctx, command_logger) = self.start_command(ops::HELLO);

        let mut res = HashMap::new();
        let mut caps = wireprotocaps();
        caps.push(format!(
            "bundle2={}",
            bundle2caps(self.support_bundle2_listkeys)
        ));
        res.insert("capabilities".to_string(), caps);

        future::ok(res)
            .timeout(*TIMEOUT)
            .map_err(process_timeout_error)
            .traced(self.session.trace(), ops::HELLO, trace_args!())
            .timed(move |stats, _| {
                command_logger.without_wireproto().finalize_command(&stats);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('listkeys', 'namespace')
    fn listkeys(&self, namespace: String) -> HgCommandRes<HashMap<Vec<u8>, Vec<u8>>> {
        if namespace == "bookmarks" {
            let (ctx, command_logger) = self.start_command(ops::LISTKEYS);

            self.get_pull_default_bookmarks_maybe_stale(ctx.clone())
                .traced(self.session.trace(), ops::LISTKEYS, trace_args!())
                .timed(move |stats, _| {
                    command_logger.without_wireproto().finalize_command(&stats);
                    Ok(())
                })
                .boxify()
        } else {
            info!(
                self.logging.logger(),
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
        if namespace != "bookmarks" {
            info!(
                self.logging.logger(),
                "unsupported listkeyspatterns namespace: {}", namespace,
            );
            return future::err(format_err!(
                "unsupported listkeyspatterns namespace: {}",
                namespace
            ))
            .boxify();
        }

        let (ctx, command_logger) = self.start_command(ops::LISTKEYSPATTERNS);

        let queries = patterns.into_iter().map({
            cloned!(ctx);
            let max = self.repo.list_keys_patterns_max();
            let repo = self.repo.blobrepo();
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
            .timeout(*TIMEOUT)
            .map_err(process_timeout_error)
            .traced(self.session.trace(), ops::LISTKEYS, trace_args!())
            .timed(move |stats, _| {
                command_logger.without_wireproto().finalize_command(&stats);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('unbundle')
    fn unbundle(
        &self,
        _heads: Vec<String>,
        stream: BoxStream<Bundle2Item, Error>,
        maybe_full_content: Option<Arc<Mutex<Bytes>>>,
    ) -> HgCommandRes<Bytes> {
        let client = self.clone();
        let pure_push_allowed = self.pure_push_allowed;
        let reponame = self.repo.reponame().clone();
        cloned!(
            self.hook_manager,
            self.cached_pull_default_bookmarks_maybe_stale,
            self.support_bundle2_listkeys
        );

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
                let (ctx, command_logger) = client.start_command(ops::UNBUNDLE);
                let blobrepo = client.repo.blobrepo().clone();
                let bookmark_attrs = client.repo.bookmark_attrs();
                let lca_hint = client.repo.lca_hint().clone();
                let phases_hint = client.repo.phases_hint().clone();
                let infinitepush_params = client.repo.infinitepush().clone();
                let infinitepush_writes_allowed = infinitepush_params.allow_writes;
                let pushrebase_params = client.repo.pushrebase_params().clone();

                let res = unbundle::resolve(
                    ctx.clone(),
                    client.repo.blobrepo().clone(),
                    infinitepush_writes_allowed,
                    stream,
                    read_write,
                    maybe_full_content,
                    pure_push_allowed,
                ).and_then({
                    cloned!(ctx);
                    move |action| {
                        run_hooks(ctx, hook_manager, &action)
                            .map(move |_| action)
                    }
                }).and_then({
                    cloned!(ctx, client, blobrepo, pushrebase_params, lca_hint, phases_hint);
                    move |action| {
                        match try_boxfuture!(client.maybe_get_push_redirector_for_action(&action)) {
                            Some(push_redirector) => {
                                let ctx = ctx.with_mutated_scuba(|mut sample| {
                                    sample.add("target_repo_name", push_redirector.repo.reponame().as_ref());
                                    sample.add("target_repo_id", push_redirector.repo.repoid().id());
                                    sample
                                });
                                ctx.scuba().clone().log_with_msg("Push redirected to large repo", None);
                                push_redirector
                                    .run_redirected_post_resolve_action_compat(ctx, action)
                                    .boxify()
                            }
                            None => run_post_resolve_action(
                                ctx,
                                blobrepo,
                                bookmark_attrs,
                                lca_hint,
                                phases_hint,
                                infinitepush_params,
                                pushrebase_params,
                                action,
                            )
                        }
                    }
                }).and_then({
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
                    cloned!(ctx, blobrepo);
                    move |response| {
                        if support_bundle2_listkeys {
                            // If this fails, we end up with a cold cache - but that just means we see the race and/or error again later
                            update_pull_default_bookmarks_maybe_stale_cache(ctx, cached_pull_default_bookmarks_maybe_stale, blobrepo)
                                .then(|_| Ok(response))
                                .left_future()
                        } else {
                            future::ok(response).right_future()
                        }
                    }
                }).and_then({
                    cloned!(ctx);
                    move |response| {
                        response.generate_bytes(
                        ctx,
                        blobrepo,
                        pushrebase_params,
                        lca_hint,
                        phases_hint)
                        .from_err()
                    }
                });

                res
                    .inspect_err({
                        cloned!(reponame);
                        move |err| {
                            use unbundle::BundleResolverError::*;
                            match err {
                                HookError((cs_hooks, file_hooks)) => {
                                    let mut failed_hooks = HashSet::new();
                                    for (exec_id, exec_info) in cs_hooks {
                                        if let HookExecution::Rejected(_) = exec_info {
                                            failed_hooks.insert(exec_id.hook_name.clone());
                                        }
                                    }
                                    for (exec_id, exec_info) in file_hooks {
                                        if let HookExecution::Rejected(_) = exec_info {
                                            failed_hooks.insert(exec_id.hook_name.clone());
                                        }
                                    }

                                    for failed_hook in failed_hooks {
                                        STATS::push_hook_failure.add_value(
                                            1, (reponame.clone(), failed_hook)
                                        );
                                    }
                                }
                                PushrebaseConflicts(..) => {
                                    STATS::push_conflicts.add_value(1, (reponame, ));
                                }
                                RateLimitExceeded { .. } => {
                                    STATS::rate_limits_exceeded.add_value(1, (reponame, ));
                                }
                                Error(..) => {
                                    STATS::push_error.add_value(1, (reponame, ));
                                }
                            };
                        }
                    })
                    .from_err()
                    .timeout(*TIMEOUT)
                    .map_err(process_timeout_error)
                    .inspect(move |_| STATS::push_success.add_value(1, (reponame, )))
                    .traced(client.session.trace(), ops::UNBUNDLE, trace_args!())
                    .timed(move |stats, _| {
                        command_logger.without_wireproto().finalize_command(&stats);
                        Ok(())
                    })
            })
            .boxify()
    }

    // @wireprotocommand('gettreepack', 'rootdir mfnodes basemfnodes directories')
    fn gettreepack(&self, params: GettreepackArgs) -> BoxStream<Bytes, Error> {
        let args = json!({
            "rootdir": String::from_utf8_lossy(&params.rootdir),
            "mfnodes": format_manifests(&params.mfnodes),
            "basemfnodes": format_manifests(&params.basemfnodes),
            "directories": format_utf8_bytes_list(&params.directories),
        });
        let args = json!(vec![args]);
        let (ctx, mut command_logger) = self.start_command(ops::GETTREEPACK);

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
    }

    // @wireprotocommand('getfiles', 'files*')
    fn getfiles(&self, params: BoxStream<(HgFileNodeId, MPath), Error>) -> BoxStream<Bytes, Error> {
        let (ctx, command_logger) = self.start_command(ops::GETFILES);
        let this = self.clone();
        // TODO(stash): make it configurable
        let getfiles_buffer_size = 100;
        // We buffer all parameters in memory so that we can log them.
        // That shouldn't be a problem because requests are quite small
        let getfiles_params = Arc::new(Mutex::new(vec![]));

        let validate_hash = rand::random::<usize>() % 100 < self.hash_validation_percentage;

        let request_stream = move || {
            cloned!(ctx);
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
                    cloned!(ctx);
                    move |(node, path)| {
                        let repo = this.repo.clone();
                        ctx.session().bump_load(Metric::EgressGetfilesFiles, 1.0);
                        create_getfiles_blob(
                            ctx.clone(),
                            repo.blobrepo().clone(),
                            node,
                            path.clone(),
                            repo.lfs_params().threshold,
                            validate_hash,
                        )
                        .traced(
                            this.session.trace(),
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
                                ctx.perf_counters().set_max_counter(
                                    PerfCounterType::GetfilesMaxLatency,
                                    completion_time,
                                );
                                Ok(())
                            }
                        })
                    }
                })
                .buffered(getfiles_buffer_size)
                .inspect({
                    cloned!(ctx);
                    move |bytes| {
                        let len = bytes.len() as i64;
                        ctx.perf_counters()
                            .add_to_counter(PerfCounterType::GetfilesResponseSize, len);
                        ctx.perf_counters()
                            .set_max_counter(PerfCounterType::GetfilesMaxFileSize, len);

                        STATS::total_fetched_file_size.add_value(len as i64);
                        if ctx.session().is_quicksand() {
                            STATS::quicksand_fetched_file_size.add_value(len as i64);
                        }
                    }
                })
                .whole_stream_timeout(*GETFILES_TIMEOUT)
                .map_err(process_stream_timeout_error)
                .timed({
                    cloned!(ctx);
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
                            .add_to_counter(PerfCounterType::GetfilesNumFiles, stats.count as i64);

                        command_logger.finalize_command(ctx, &stats, Some(&json! {encoded_params}));

                        Ok(())
                    }
                })
                .boxify()
        };

        throttle_stream(
            &self.session,
            Metric::EgressGetfilesFiles,
            ops::GETFILES,
            request_stream,
        )
    }

    // @wireprotocommand('stream_out_shallow')
    fn stream_out_shallow(&self) -> BoxStream<Bytes, Error> {
        let (ctx, command_logger) = self.start_command(ops::STREAMOUTSHALLOW);
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
                                move |stats, _| {
                                    ctx.perf_counters().add_to_counter(
                                        PerfCounterType::SumManifoldPollTime,
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
                                        PerfCounterType::SumManifoldPollTime,
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
            .whole_stream_timeout(*CLONE_TIMEOUT)
            .map_err(process_stream_timeout_error)
            .timed({
                move |stats, _| {
                    command_logger.finalize_command(ctx, &stats, None);
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
    ) -> BoxStream<Bytes, Error> {
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
    if !params.directories.is_empty() {
        // This param is not used by core hg, don't worry about implementing it now
        return stream::once(Err(Error::msg("directories param is not supported"))).boxify();
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
    max_depth: usize,
) -> BoxStream<(HgManifestId, Option<MPath>), Error> {
    if max_depth == 1 {
        return stream::iter_ok(vec![(mfid, rootpath)]).boxify();
    }

    basemfid
        .filtered_diff(
            ctx.clone(),
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

    repo.get_filenode_opt(
        ctx.clone(),
        &repo_path,
        HgFileNodeId::new(hg_mf_id.into_nodehash()),
    )
    .join(envelope_fut)
    .map(move |(maybe_filenode, envelope)| {
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
                let (p1, p2) = envelope.parents();
                let parents = HgParents::new(p1, p2);

                (parents, NULL_CSID, content)
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
            content,
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

#[cfg(test)]
mod tests;
