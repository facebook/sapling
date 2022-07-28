/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::errors::ErrorKind;

use unbundle::run_hooks;
use unbundle::run_post_resolve_action;
use unbundle::BundleResolverError;
use unbundle::CrossRepoPushSource;
use unbundle::PushRedirector;
use unbundle::PushRedirectorArgs;

use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobrepo::AsBlobRepo;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bookmarks::Bookmark;
use bookmarks::BookmarkName;
use bookmarks::BookmarkPrefix;
use bookmarks_types::BookmarkKind;
use bytes::Bytes;
use bytes_old::BufMut as BufMutOld;
use bytes_old::Bytes as BytesOld;
use bytes_old::BytesMut as BytesMutOld;
use cloned::cloned;
use context::CoreContext;
use context::LoggingContainer;
use context::PerfCounterType;
use context::PerfCounters;
use context::SessionContainer;
use filenodes::FilenodeResult;
use futures::channel::oneshot;
use futures::channel::oneshot::Sender;
use futures::compat::Future01CompatExt;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_01_ext::try_boxstream;
use futures_01_ext::BoxFuture;
use futures_01_ext::BoxStream;
use futures_01_ext::FutureExt as OldFutureExt;
use futures_01_ext::StreamExt as OldStreamExt;
use futures_ext::stream::FbTryStreamExt;
use futures_ext::BufferedParams;
use futures_ext::FbFutureExt;
use futures_ext::FbStreamExt;
use futures_ext::FbTryFutureExt;
use futures_old::future as future_old;
use futures_old::future::ok;
use futures_old::stream as stream_old;
use futures_old::try_ready;
use futures_old::Async;
use futures_old::Future;
use futures_old::IntoFuture;
use futures_old::Poll;
use futures_old::Stream;
use futures_stats::TimedFutureExt;
use futures_stats::TimedStreamExt;
use getbundle_response::create_getbundle_response;
use getbundle_response::DraftsInBundlesPolicy;
use getbundle_response::PhasesPart;
use getbundle_response::SessionLfsParams;
use hgproto::GetbundleArgs;
use hgproto::GettreepackArgs;
use hgproto::HgCommandRes;
use hgproto::HgCommands;
use hostname::get_hostname;
use itertools::Itertools;
use lazy_static::lazy_static;
use manifest::Diff;
use manifest::Entry;
use manifest::ManifestOps;
use maplit::hashmap;
use mercurial_bundles::create_bundle_stream;
use mercurial_bundles::parts;
use mercurial_bundles::wirepack;
use mercurial_bundles::Bundle2Item;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_revlog::RevlogChangeset;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::calculate_hg_node_id;
use mercurial_types::convert_parents_to_remotefilelog_format;
use mercurial_types::fetch_manifest_envelope;
use mercurial_types::percent_encode;
use mercurial_types::Delta;
use mercurial_types::HgChangesetId;
use mercurial_types::HgChangesetIdPrefix;
use mercurial_types::HgChangesetIdsResolvedFromPrefix;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mercurial_types::HgParents;
use mercurial_types::MPath;
use mercurial_types::RepoPath;
use mercurial_types::NULL_CSID;
use mercurial_types::NULL_HASH;
use metaconfig_types::RepoClientKnobs;
use metaconfig_types::RepoConfigRef;
use mononoke_api::Repo;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use nonzero_ext::nonzero;
use phases::PhasesArc;
use rand::Rng;
use rate_limiting::Metric;
use reachabilityindex::LeastCommonAncestorsHint;
use regex::Regex;
use remotefilelog::create_getpack_v1_blob;
use remotefilelog::create_getpack_v2_blob;
use remotefilelog::get_unordered_file_history_for_multiple_nodes;
use remotefilelog::GetpackBlobInfo;
use repo_identity::RepoIdentityRef;
use revisionstore_types::Metadata;
use serde::Deserialize;
use serde_json::json;
use skiplist::SkiplistIndexArc;
use slog::debug;
use slog::error;
use slog::info;
use slog::o;
use stats::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write;
use std::hash::Hash;
use std::hash::Hasher;
use std::mem;
use std::num::NonZeroU64;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;
use streaming_clone::RevlogStreamingChunks;
use streaming_clone::StreamingCloneArc;
use time_ext::DurationExt;
use tunables::tunables;

mod logging;
mod monitor;
mod session_bookmarks_cache;
mod tests;

use logging::debug_format_manifest;
use logging::debug_format_path;
use logging::log_getpack_params_verbose;
use logging::log_gettreepack_params_verbose;
use logging::CommandLogger;
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

#[derive(Clone, Copy, Debug)]
struct SamplingRate(NonZeroU64);

const GETTREEPACK_FEW_MFNODES_SAMPLING_RATE: SamplingRate = SamplingRate(nonzero!(100u64));
const UNSAMPLED: SamplingRate = SamplingRate(nonzero!(1u64));

fn gettreepack_scuba_sampling_rate(params: &GettreepackArgs) -> SamplingRate {
    if params.mfnodes.len() == 1 {
        GETTREEPACK_FEW_MFNODES_SAMPLING_RATE
    } else {
        UNSAMPLED
    }
}

fn debug_format_manifests<'a>(nodes: impl IntoIterator<Item = &'a HgManifestId>) -> String {
    nodes.into_iter().map(debug_format_manifest).join(" ")
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

lazy_static! {
    static ref SLOW_REQUEST_THRESHOLD: Duration = Duration::from_secs(1);
}

fn clone_timeout() -> Duration {
    let timeout = tunables().get_repo_client_clone_timeout_secs();
    if timeout > 0 {
        Duration::from_secs(timeout as u64)
    } else {
        Duration::from_secs(4 * 60 * 60)
    }
}

fn default_timeout() -> Duration {
    let timeout = tunables().get_repo_client_default_timeout_secs();
    if timeout > 0 {
        Duration::from_secs(timeout as u64)
    } else {
        Duration::from_secs(15 * 60)
    }
}
fn getbundle_timeout() -> Duration {
    let timeout = tunables().get_repo_client_getbundle_timeout_secs();
    if timeout > 0 {
        Duration::from_secs(timeout as u64)
    } else {
        Duration::from_secs(30 * 60)
    }
}

fn getpack_timeout() -> Duration {
    let timeout = tunables().get_repo_client_getpack_timeout_secs();
    if timeout > 0 {
        Duration::from_secs(timeout as u64)
    } else {
        Duration::from_secs(5 * 60 * 60)
    }
}

fn wireprotocaps() -> Vec<String> {
    vec![
        "clienttelemetry".to_string(),
        "lookup".to_string(),
        "known".to_string(),
        "getbundle".to_string(),
        "unbundle=HG10GZ,HG10BZ,HG10UN".to_string(),
        "unbundlereplay".to_string(),
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
        if !value.is_empty() {
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
    path_regex_to_log: Option<Regex>,
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

        let path_regex_to_log = if repo_needs_logging
            && !tunables.get_undesired_path_regex_to_log().is_empty()
        {
            Some(
                Regex::new(tunables.get_undesired_path_regex_to_log().as_str()).map_err(|e| {
                    error!(
                        ctx.logger(),
                        "Error initializing undesired path regex for {}: {}",
                        repo.name(),
                        e
                    );
                    e
                })?,
            )
        } else {
            None
        };

        Ok(Self {
            ctx,
            repo_needs_logging,
            path_prefix_to_log,
            path_regex_to_log,
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
                    .scuba()
                    .clone()
                    .add("undesired_file_size", size)
                    .log_with_msg("Undesired file fetch", format!("{:?}", path));
            }
        }
    }

    fn should_log(&self, path: Option<&MPath>) -> bool {
        if self.repo_needs_logging {
            let op1 = match self.path_prefix_to_log.as_ref() {
                None => false,
                Some(prefix) => prefix.is_prefix_of(MPath::iter_opt(path)),
            };

            let op2 = match (path, self.path_regex_to_log.as_ref()) {
                (Some(path), Some(re)) => path.matches_regex(re),
                _ => false,
            };

            op1 || op2
        } else {
            false
        }
    }
}

#[derive(Clone)]
pub struct RepoClient {
    repo: Arc<Repo>,
    // The session for this repo access.
    session: SessionContainer,
    // A base logging container. This will be combined with the Session container for each command
    // to produce a CoreContext.
    logging: LoggingContainer,
    // There is a race condition in bookmarks handling in Mercurial, which needs protocol-level
    // fixes. See `test-bookmark-race.t` for a reproducer; the issue is that between discovery
    // and bookmark handling (listkeys), we can get new commits and a bookmark change.
    // The client then gets a bookmark that points to a commit it does not yet have, and ignores it.
    // We currently fix it by caching bookmarks at the beginning of discovery.
    // TODO: T45411456 Fix this by teaching the client to expect extra commits to correspond to the bookmarks.
    session_bookmarks_cache: Arc<SessionBookmarkCache>,
    maybe_push_redirector_args: Option<PushRedirectorArgs>,
    force_lfs: Arc<AtomicBool>,
    unhydrated_commits: Arc<AtomicBool>,
    knobs: RepoClientKnobs,
    request_perf_counters: Arc<PerfCounters>,
    // In case `repo` is a backup of another repository `maybe_backup_repo_source` points to
    // a source for this repository.
    maybe_backup_repo_source: Option<BlobRepo>,
}

impl RepoClient {
    pub fn new(
        repo: Arc<Repo>,
        session: SessionContainer,
        logging: LoggingContainer,
        maybe_push_redirector_args: Option<PushRedirectorArgs>,
        knobs: RepoClientKnobs,
        maybe_backup_repo_source: Option<BlobRepo>,
    ) -> Self {
        let session_bookmarks_cache = Arc::new(SessionBookmarkCache::new(repo.clone()));

        Self {
            repo,
            session,
            logging,
            session_bookmarks_cache,
            maybe_push_redirector_args,
            force_lfs: Arc::new(AtomicBool::new(false)),
            unhydrated_commits: Arc::new(AtomicBool::new(false)),
            knobs,
            request_perf_counters: Arc::new(PerfCounters::default()),
            maybe_backup_repo_source,
        }
    }

    pub fn request_perf_counters(&self) -> Arc<PerfCounters> {
        self.request_perf_counters.clone()
    }

    fn command_future<F, I, E, H>(
        &self,
        command: &str,
        sampling_rate: SamplingRate,
        handler: H,
    ) -> BoxFuture<I, E>
    where
        F: Future<Item = I, Error = E> + Send + 'static,
        H: FnOnce(CoreContext, CommandLogger) -> F,
    {
        let (ctx, command_logger) = self.start_command(command, sampling_rate);
        with_command_monitor(ctx.clone(), handler(ctx, command_logger)).boxify()
    }

    fn command_stream<S, I, E, H>(
        &self,
        command: &str,
        sampling_rate: SamplingRate,
        handler: H,
    ) -> BoxStream<I, E>
    where
        S: Stream<Item = I, Error = E> + Send + 'static,
        H: FnOnce(CoreContext, CommandLogger) -> S,
    {
        let (ctx, command_logger) = self.start_command(command, sampling_rate);
        with_command_monitor(ctx.clone(), handler(ctx, command_logger)).boxify()
    }

    fn start_command(
        &self,
        command: &str,
        sampling_rate: SamplingRate,
    ) -> (CoreContext, CommandLogger) {
        info!(self.logging.logger(), "{}", command);

        let logger = self
            .logging
            .logger()
            .new(o!("command" => command.to_owned()));

        let mut scuba = self.logging.scuba().clone();
        scuba
            .sampled_unless_verbose(sampling_rate.0)
            .add("command", command);
        scuba.clone().log_with_msg("Start processing", None);

        let ctx =
            self.session
                .new_context_with_scribe(logger, scuba, self.logging.scribe().clone());

        let command_logger = CommandLogger::new(ctx.clone(), self.request_perf_counters.clone());

        (ctx, command_logger)
    }

    fn get_publishing_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = HashMap<Bookmark, HgChangesetId>, Error = Error> {
        let session_bookmarks_cache = self.session_bookmarks_cache.clone();
        (async move { session_bookmarks_cache.get_publishing_bookmarks(ctx).await })
            .boxed()
            .compat()
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
        let blobrepo = self.repo.blob_repo().clone();
        let reponame = self.repo.inner_repo().repo_identity().name().to_string();
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
        let lca_hint: Arc<dyn LeastCommonAncestorsHint> =
            self.repo.inner_repo().skiplist_index_arc();

        let drafts_in_bundles_policy = if self
            .repo
            .inner_repo()
            .repo_config()
            .infinitepush
            .hydrate_getbundle_response
            && !self.unhydrated_commits_requested()
        {
            DraftsInBundlesPolicy::WithTreesAndFiles
        } else {
            DraftsInBundlesPolicy::CommitsOnly
        };

        async move {
            create_getbundle_response(
                &ctx,
                &blobrepo,
                &reponame,
                common,
                &heads,
                &lca_hint,
                if use_phases {
                    PhasesPart::Yes
                } else {
                    PhasesPart::No
                },
                &lfs_params,
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
                    .map(stream_old::iter_ok)
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
        let hash_validation_percentage = tunables().get_hash_validation_percentage();
        let validate_hash = ((rand::random::<usize>() % 100) as i64) < hash_validation_percentage;

        let undesired_path_logger =
            try_boxstream!(UndesiredPathLogger::new(ctx.clone(), self.repo.blob_repo()));

        let changed_entries = gettreepack_entries(ctx.clone(), self.repo.blob_repo(), params)
            .filter({
                let mut used_hashes = HashSet::new();
                move |(hg_mf_id, _)| used_hashes.insert(hg_mf_id.clone())
            })
            .map({
                cloned!(ctx);
                let blobrepo = self.repo.blob_repo().clone();
                move |(hg_mf_id, path)| {
                    undesired_path_logger.maybe_log_tree(path.as_ref());

                    ctx.perf_counters()
                        .increment_counter(PerfCounterType::GettreepackNumTreepacks);

                    ctx.session().bump_load(Metric::TotalManifests, 1.0);
                    STATS::total_tree_count.add_value(1);
                    if ctx.session().is_quicksand() {
                        STATS::quicksand_tree_count.add_value(1);
                    }
                    fetch_treepack_part_input(ctx.clone(), &blobrepo, hg_mf_id, path, validate_hash)
                }
            });

        let part = parts::treepack_part(changed_entries, parts::StoreInHgCache::Yes);
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
        WeightedContent: Future<Item = (GetpackBlobInfo, Content), Error = Error> + Send + 'static,
        Content:
            Future<Item = (HgFileNodeId, Bytes, Option<Metadata>), Error = Error> + Send + 'static,
        GetpackHandler: Fn(CoreContext, BlobRepo, HgFileNodeId, SessionLfsParams, bool) -> WeightedContent
            + Send
            + 'static,
    {
        let allow_short_getpack_history = self.knobs.allow_short_getpack_history;
        self.command_stream(name, UNSAMPLED, |ctx, command_logger| {
            let undesired_path_logger =
                try_boxstream!(UndesiredPathLogger::new(ctx.clone(), self.repo.blob_repo()));
            let undesired_path_logger = Arc::new(undesired_path_logger);
            // We buffer all parameters in memory so that we can log them.
            // That shouldn't be a problem because requests are quite small
            let getpack_params = Arc::new(Mutex::new(vec![]));
            let repo = self.repo.blob_repo().clone();

            let lfs_params = self.lfs_params();

            let hash_validation_percentage = tunables().get_hash_validation_percentage();
            let validate_hash =
                rand::thread_rng().gen_ratio(hash_validation_percentage as u32, 100);
            let getpack_buffer_size = 500;

            let request_stream = move || {
                let content_stream = {
                    cloned!(ctx, getpack_params, lfs_params, undesired_path_logger);

                    async move {
                        let buffered_params = BufferedParams {
                            weight_limit: 100_000_000,
                            buffer_size: getpack_buffer_size,
                        };

                        // Let's fetch the whole request before responding.
                        // That's prevents deadlocks, because hg client doesn't start reading the response
                        // before all the arguments were sent.
                        let params = params.compat().try_collect::<Vec<_>>().await?;

                        ctx.scuba()
                            .clone()
                            .add("getpack_paths", params.len())
                            .log_with_msg("Getpack Params", None);

                        let res = stream::iter(params.into_iter())
                            .map({
                                cloned!(ctx, getpack_params, repo, lfs_params);
                                move |(path, filenodes)| {
                                    {
                                        let mut getpack_params = getpack_params.lock().unwrap();
                                        getpack_params.push((path.clone(), filenodes.clone()));
                                    }

                                    ctx.session().bump_load(Metric::GetpackFiles, 1.0);

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
                                            .compat()
                                        })
                                        .collect();

                                    // NOTE: We don't otherwise await history_fut until we have the results
                                    // from blob_futs, so we need to spawn this to start fetching history
                                    // before we have resoved hg filenodes.
                                    let history_fut = tokio::task::spawn(
                                        get_unordered_file_history_for_multiple_nodes(
                                            ctx.clone(),
                                            repo.clone(),
                                            filenodes.into_iter().collect(),
                                            &path,
                                            allow_short_getpack_history,
                                        )
                                        .compat()
                                        .try_collect::<Vec<_>>(),
                                    )
                                    .flatten_err();

                                    cloned!(undesired_path_logger);

                                    async move {
                                        let blobs =
                                            future::try_join_all(blob_futs.into_iter()).await?;

                                        undesired_path_logger.maybe_log_file(
                                            Some(&path),
                                            blobs.iter().map(|(blobinfo, _)| blobinfo.filesize),
                                        );

                                        let total_weight = blobs
                                            .iter()
                                            .map(|(blob_info, _)| blob_info.weight)
                                            .sum();
                                        let content_futs =
                                            blobs.into_iter().map(|(_, fut)| fut.compat());
                                        let contents_and_history = future::try_join(
                                            future::try_join_all(content_futs),
                                            history_fut,
                                        )
                                        .map_ok(move |(contents, history)| {
                                            (path, contents, history)
                                        });

                                        Result::<_, Error>::Ok((contents_and_history, total_weight))
                                    }
                                }
                            })
                            .buffered(getpack_buffer_size)
                            .try_buffered_weight_limited(buffered_params);

                        Result::<_, Error>::Ok(res)
                    }
                }
                .try_flatten_stream();

                let serialized_stream = content_stream
                    .whole_stream_timeout(getpack_timeout())
                    .yield_periodically()
                    .flatten_err()
                    .boxed()
                    .compat()
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
                            stream_old::iter_ok(res.into_iter())
                        }
                    })
                    .flatten()
                    .chain(stream_old::once(Ok(wirepack::Part::End)));

                wirepack::packer::WirePackPacker::new(serialized_stream, wirepack::Kind::File)
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
                    .boxify()
                    .compat()
                    .timed({
                        cloned!(ctx);
                        move |stats| {
                            STATS::getpack_ms
                                .add_value(stats.completion_time.as_millis_unchecked() as i64);
                            let encoded_params = {
                                let getpack_params = getpack_params.lock().unwrap();
                                let mut encoded_params: Vec<(String, Vec<String>)> = vec![];
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

                            log_getpack_params_verbose(&ctx, &encoded_params);
                            command_logger.finalize_command(&stats);

                            future::ready(())
                        }
                    })
                    .boxed()
                    .compat()
            };

            throttle_stream(&self.session, Metric::GetpackFiles, name, request_stream).boxify()
        })
    }

    fn lfs_params(&self) -> SessionLfsParams {
        if self.force_lfs.load(Ordering::Relaxed) {
            SessionLfsParams {
                threshold: self.repo.inner_repo().repo_config().lfs.threshold,
            }
        } else {
            let client_hostname = self.session.metadata().client_hostname();
            let percentage = self.repo.inner_repo().repo_config().lfs.rollout_percentage;

            let allowed = match client_hostname {
                Some(client_hostname) => {
                    let mut hasher = DefaultHasher::new();
                    client_hostname.hash(&mut hasher);
                    hasher.finish() % 100 < percentage.into()
                }
                None => {
                    // Randomize in case source hostname is not set to avoid
                    // sudden jumps in traffic
                    rand::thread_rng().gen_ratio(percentage, 100)
                }
            };

            if allowed {
                SessionLfsParams {
                    threshold: self.repo.inner_repo().repo_config().lfs.threshold,
                }
            } else {
                SessionLfsParams { threshold: None }
            }
        }
    }

    fn unhydrated_commits_requested(&self) -> bool {
        self.unhydrated_commits.load(Ordering::Relaxed)
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

        use unbundle::PostResolveAction::*;

        let live_commit_sync_config = self.repo.live_commit_sync_config();

        let repo_id = self.repo.blob_repo().get_repoid();
        let redirect = match action {
            InfinitePush(_) => live_commit_sync_config.push_redirector_enabled_for_draft(repo_id),
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
                live_commit_sync_config,
                self.repo.x_repo_sync_lease().clone(),
            )?))
        } else {
            debug!(
                ctx.logger(),
                "live_commit_sync_config says push redirection is off"
            );
            Ok(None)
        }
    }

    fn known_impl<Func, Fut>(
        &self,
        nodes: Vec<HgChangesetId>,
        command: &'static str,
        filter: Func,
    ) -> HgCommandRes<Vec<bool>>
    where
        Func: FnOnce(CoreContext, Vec<HgChangesetId>, Vec<(HgChangesetId, ChangesetId)>) -> Fut
            + Send
            + 'static,
        Fut: future::Future<Output = Result<Vec<bool>, Error>> + Send + 'static,
    {
        self.command_future(command, UNSAMPLED, |ctx, mut command_logger| {
            let blobrepo = self.repo.blob_repo().clone();

            let nodes_len = nodes.len();
            ctx.perf_counters()
                .add_to_counter(PerfCounterType::NumKnownRequested, nodes_len as i64);
            let args = json!({
                "nodes_count": nodes_len,
            });

            command_logger.add_trimmed_scuba_extra("command_args", &args);

            {
                cloned!(ctx);
                async move {
                    let max_nodes = tunables()
                        .get_repo_client_max_nodes_in_known_method()
                        .try_into()
                        .unwrap();
                    if max_nodes > 0 {
                        if nodes_len > max_nodes {
                            return Err(format_err!(
                                "invalid request - too many requests were sent in 'known' method"
                            ));
                        }
                    }
                    let hg_bcs_mapping = blobrepo
                        .get_hg_bonsai_mapping(ctx.clone(), nodes.clone())
                        .await?;

                    filter(ctx, nodes, hg_bcs_mapping).await
                }
            }
            .timeout(default_timeout())
            .flatten_err()
            .timed()
            .map(move |(stats, known_nodes)| {
                if let Ok(ref known) = known_nodes {
                    ctx.perf_counters()
                        .add_to_counter(PerfCounterType::NumKnown, known.len() as i64);
                    ctx.perf_counters().add_to_counter(
                        PerfCounterType::NumUnknown,
                        (nodes_len - known.len()) as i64,
                    );
                }
                command_logger.without_wireproto().finalize_command(&stats);
                known_nodes
            })
            .boxed()
            .compat()
        })
    }
}

fn throttle_stream<F, S, V>(
    session: &SessionContainer,
    metric: Metric,
    request_name: &'static str,
    func: F,
) -> impl Stream<Item = V, Error = Error>
where
    F: FnOnce() -> S + Send + 'static,
    S: Stream<Item = V, Error = Error> + Send + 'static,
{
    let session = session.clone();
    async move {
        session
            .check_rate_limit(metric)
            .await
            .map_err(|reason| ErrorKind::RequestThrottled {
                request_name: request_name.into(),
                reason,
            })?;

        Result::<_, Error>::Ok(func())
    }
    .boxed()
    .compat()
    .flatten_stream()
}

impl HgCommands for RepoClient {
    // @wireprotocommand('between', 'pairs')
    fn between(
        &self,
        pairs: Vec<(HgChangesetId, HgChangesetId)>,
    ) -> HgCommandRes<Vec<Vec<HgChangesetId>>> {
        struct ParentStream<CS> {
            ctx: CoreContext,
            repo: Arc<Repo>,
            n: HgChangesetId,
            bottom: HgChangesetId,
            wait_cs: Option<CS>,
        }

        impl<CS> ParentStream<CS> {
            fn new(
                ctx: CoreContext,
                repo: &Arc<Repo>,
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
                        {
                            cloned!(self.n, self.ctx, self.repo);
                            async move { n.load(&ctx, repo.blob_repo().blobstore()).await }
                        }
                        .boxed()
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

        self.command_future(ops::BETWEEN, UNSAMPLED, |ctx, command_logger| {
            // TODO(jsgf): do pairs in parallel?
            // TODO: directly return stream of streams
            cloned!(self.repo);
            stream_old::iter_ok(pairs.into_iter())
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
                .compat()
                .timeout(default_timeout())
                .flatten_err()
                .timed()
                .map(move |(stats, res)| {
                    command_logger.without_wireproto().finalize_command(&stats);
                    res
                })
                .boxed()
                .compat()
        })
    }

    // @wireprotocommand('clienttelemetry')
    fn clienttelemetry(&self, args: HashMap<Vec<u8>, Vec<u8>>) -> HgCommandRes<String> {
        self.command_future(
            ops::CLIENTTELEMETRY,
            UNSAMPLED,
            |ctx, mut command_logger| {
                let hostname = match get_hostname() {
                    Err(_) => format!("session {}", ctx.metadata().session_id()),
                    Ok(host) => format!("{} session {}", host, ctx.metadata().session_id()),
                };

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

                if let Some(val) = args.get(b"wantsunhydratedcommits" as &[u8]) {
                    if val == b"True" {
                        self.unhydrated_commits.store(true, Ordering::Relaxed);
                    }
                }

                future::ok(hostname)
                    .timed()
                    .map(move |(stats, res)| {
                        command_logger.without_wireproto().finalize_command(&stats);
                        res
                    })
                    .compat()
            },
        )
    }

    // @wireprotocommand('heads')
    fn heads(&self) -> HgCommandRes<HashSet<HgChangesetId>> {
        // Get a stream of heads and collect them into a HashSet
        // TODO: directly return stream of heads
        self.command_future(ops::HEADS, UNSAMPLED, |ctx, command_logger| {
            // Heads are all the commits that has a publishing bookmarks
            // that points to it.
            self.get_publishing_bookmarks_maybe_stale(ctx)
                .map(|map| map.into_iter().map(|(_, hg_cs_id)| hg_cs_id).collect())
                .compat()
                .timeout(default_timeout())
                .flatten_err()
                .timed()
                .map(move |(stats, res)| {
                    command_logger.without_wireproto().finalize_command(&stats);
                    res
                })
                .boxed()
                .compat()
        })
    }

    // @wireprotocommand('lookup', 'key')
    fn lookup(&self, key: String) -> HgCommandRes<BytesOld> {
        // Generate positive response including HgChangesetId as hex.
        fn generate_changeset_resp_buf(csid: HgChangesetId) -> HgCommandRes<BytesOld> {
            Ok(generate_lookup_resp_buf(true, csid.to_hex().as_bytes()))
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
                    cloned!(ctx, repo);
                    async move { hg_csid.load(&ctx, repo.blobstore()).await }
                        .boxed()
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
                    generate_lookup_resp_buf(false, &infos.join(&[b'\n'][..]))
                })
                .boxify()
        }

        // Controls how many suggestions to fetch in case of ambiguous outcome of prefix lookup.
        const MAX_NUMBER_OF_SUGGESTIONS_TO_FETCH: usize = 10;

        let maybe_git_lookup = parse_git_lookup(&key);
        self.command_future(ops::LOOKUP, UNSAMPLED, |ctx, command_logger| {
            let repo = self.repo.blob_repo().clone();

            // Resolves changeset or set of suggestions from the key (full hex hash or a prefix) if exist.
            // Note: `get_many_hg_by_prefix` works for the full hex hashes well but
            //       `changeset_exists` has better caching and is preferable for the full length hex hashes.
            let node_fut = match HgChangesetId::from_str(&key) {
                Ok(csid) => {
                    cloned!(ctx, repo);
                    async move { repo.changeset_exists(ctx, csid).await }
                }
                .boxed()
                .compat()
                .map(move |exists| {
                    if exists {
                        HgChangesetIdsResolvedFromPrefix::Single(csid)
                    } else {
                        HgChangesetIdsResolvedFromPrefix::NoMatch
                    }
                })
                .boxify(),
                Err(_) => match HgChangesetIdPrefix::from_str(&key) {
                    Ok(cs_prefix) => {
                        cloned!(repo, ctx);
                        async move {
                            repo.bonsai_hg_mapping()
                                .get_many_hg_by_prefix(
                                    &ctx,
                                    cs_prefix,
                                    MAX_NUMBER_OF_SUGGESTIONS_TO_FETCH,
                                )
                                .await
                        }
                        .boxed()
                        .compat()
                        .boxify()
                    }
                    Err(_) => ok(HgChangesetIdsResolvedFromPrefix::NoMatch).boxify(),
                },
            };

            // The lookup order:
            // If there is a git_lookup match, return that.
            // If there is an exact commit match, return that even if the key is the prefix of the hash.
            // If there is a bookmark match, return that.
            // If there are suggestions, show them. This happens in case of ambiguous outcome of prefix lookup.
            // Otherwise, show an error.

            let bookmark = BookmarkName::new(&key).ok();
            let lookup_fut = node_fut
                .and_then({
                    cloned!(ctx, repo);
                    move |resolved_cids| {
                        use HgChangesetIdsResolvedFromPrefix::*;

                        // Describing the priority relative to bookmark presence for the key.
                        enum LookupOutcome {
                            HighPriority(HgCommandRes<BytesOld>),
                            LowPriority(HgCommandRes<BytesOld>),
                        }

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
                                Ok(generate_lookup_resp_buf(
                                    false,
                                    format!("ambiguous identifier '{}'", key).as_bytes(),
                                ))
                                .into_future()
                                .boxify(),
                            ),
                            NoMatch => LookupOutcome::LowPriority(
                                Ok(generate_lookup_resp_buf(
                                    false,
                                    format!("{} not found", key).as_bytes(),
                                ))
                                .into_future()
                                .boxify(),
                            ),
                        };

                        match (outcome, bookmark) {
                            (LookupOutcome::HighPriority(res), _) => res,
                            (LookupOutcome::LowPriority(res), Some(bookmark)) => {
                                async move { repo.get_bookmark(ctx.clone(), &bookmark).await }
                                    .boxed()
                                    .compat()
                                    .and_then(move |maybe_csid| {
                                        if let Some(csid) = maybe_csid {
                                            generate_changeset_resp_buf(csid)
                                        } else {
                                            res
                                        }
                                    })
                                    .boxify()
                            }
                            (LookupOutcome::LowPriority(res), None) => res,
                        }
                    }
                })
                .boxify();

            async move {
                if let Some(git_lookup) = maybe_git_lookup {
                    if let Some(res) = git_lookup.lookup(&ctx, &repo).await? {
                        return Ok(res);
                    }
                }
                lookup_fut.compat().await
            }
            .timeout(default_timeout())
            .flatten_err()
            .timed()
            .map(move |(stats, res)| {
                command_logger.without_wireproto().finalize_command(&stats);
                res
            })
            .boxed()
            .compat()
        })
    }

    // @wireprotocommand('known', 'nodes *'), but the '*' is ignored
    fn known(&self, nodes: Vec<HgChangesetId>) -> HgCommandRes<Vec<bool>> {
        let phases_hint = self.repo.inner_repo().phases_arc();
        self.known_impl(
            nodes,
            ops::KNOWN,
            move |ctx, nodes, hg_bcs_mapping| async move {
                let mut bcs_ids = vec![];
                let mut bcs_hg_mapping = hashmap! {};

                for (hg, bcs) in hg_bcs_mapping {
                    bcs_ids.push(bcs);
                    bcs_hg_mapping.insert(bcs, hg);
                }

                let found_hg_changesets = phases_hint
                    .get_public(&ctx, bcs_ids, false)
                    .map_ok(move |public_csids| {
                        public_csids
                            .into_iter()
                            .filter_map(|csid| bcs_hg_mapping.get(&csid).cloned())
                            .collect::<HashSet<_>>()
                    })
                    .await?;

                let res = nodes
                    .into_iter()
                    .map(move |node| found_hg_changesets.contains(&node))
                    .collect::<Vec<_>>();

                Ok(res)
            },
        )
    }

    fn knownnodes(&self, nodes: Vec<HgChangesetId>) -> HgCommandRes<Vec<bool>> {
        self.known_impl(
            nodes,
            ops::KNOWNNODES,
            move |_ctx, nodes, hg_bcs_mapping| async move {
                let hg_bcs_mapping = hg_bcs_mapping.into_iter().collect::<HashMap<_, _>>();
                let res = nodes
                    .into_iter()
                    .map(move |node| hg_bcs_mapping.contains_key(&node))
                    .collect::<Vec<_>>();

                Ok(res)
            },
        )
    }

    // @wireprotocommand('getbundle', '*')
    fn getbundle(&self, args: GetbundleArgs) -> BoxStream<BytesOld, Error> {
        self.command_stream(ops::GETBUNDLE, UNSAMPLED, |ctx, command_logger| {
            let s = self
                .create_bundle(ctx, args)
                .compat()
                .whole_stream_timeout(getbundle_timeout())
                .yield_periodically()
                .flatten_err()
                .timed({
                    move |stats| {
                        STATS::getbundle_ms
                            .add_value(stats.completion_time.as_millis_unchecked() as i64);
                        command_logger.finalize_command(&stats);
                        future::ready(())
                    }
                })
                .boxed()
                .compat()
                .boxify();

            throttle_stream(&self.session, Metric::Commits, ops::GETBUNDLE, move || s)
        })
    }

    // @wireprotocommand('hello')
    fn hello(&self) -> HgCommandRes<HashMap<String, Vec<String>>> {
        self.command_future(ops::HELLO, UNSAMPLED, |_ctx, command_logger| {
            let mut res = HashMap::new();
            let mut caps = wireprotocaps();
            caps.push(format!("bundle2={}", bundle2caps()));
            res.insert("capabilities".to_string(), caps);

            future::ok(res)
                .timed()
                .map(move |(stats, res)| {
                    command_logger.without_wireproto().finalize_command(&stats);
                    res
                })
                .boxed()
                .compat()
                .boxify()
        })
    }

    // @wireprotocommand('listkeys', 'namespace')
    fn listkeys(&self, namespace: String) -> HgCommandRes<HashMap<Vec<u8>, Vec<u8>>> {
        if namespace == "bookmarks" {
            self.command_future(ops::LISTKEYS, UNSAMPLED, |ctx, command_logger| {
                self.get_pull_default_bookmarks_maybe_stale(ctx)
                    .compat()
                    .timed()
                    .map(move |(stats, res)| {
                        command_logger.without_wireproto().finalize_command(&stats);
                        res
                    })
                    .compat()
                    .boxify()
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

        self.command_future(ops::LISTKEYSPATTERNS, UNSAMPLED, |ctx, command_logger| {
            let max = self.repo.inner_repo().repo_config().list_keys_patterns_max;
            let session_bookmarks_cache = self.session_bookmarks_cache.clone();

            let queries = patterns.into_iter().map(move |pattern| {
                cloned!(ctx, session_bookmarks_cache);
                async move {
                    if pattern.ends_with('*') {
                        // prefix match
                        let prefix = BookmarkPrefix::new(&pattern[..pattern.len() - 1])?;

                        let bookmarks = session_bookmarks_cache
                            .get_bookmarks_by_prefix(&ctx, &prefix, max).await?
                            .map_ok(|(bookmark, cs_id)| {
                                (bookmark.to_string(), cs_id)
                            })
                            .try_collect::<Vec<_>>().await?;

                        if bookmarks.len() < max as usize {
                            Ok(bookmarks)
                        } else {
                            Err(format_err!(
                                    "Bookmark query was truncated after {} results, use a more specific prefix search.",
                                    max,
                            ))
                        }
                    } else {
                        // literal match
                        let bookmark = BookmarkName::new(&pattern)?;

                        let cs_id = session_bookmarks_cache.get_bookmark(ctx, bookmark).await?;
                        match cs_id {
                            Some(cs_id) => Ok(vec![(pattern, cs_id)]),
                            None => Ok(Vec::new()),
                        }
                    }
                }
            });

            queries
                .collect::<FuturesUnordered<_>>()
                .try_fold(BTreeMap::new(), |mut ret, books| {
                    ret.extend(books);
                    future::ready(Ok(ret))
                })
                .timeout(default_timeout())
                .flatten_err()
                .timed()
                .map(move |(stats, res)| {
                    command_logger.without_wireproto().finalize_command(&stats);
                    res
                })
                .boxed()
                .compat()
        })
    }

    // @wireprotocommand('unbundle')
    fn unbundle(
        &self,
        _heads: Vec<String>,
        stream: BoxStream<Bundle2Item<'static>, Error>,
        respondlightly: Option<bool>,
        maybereplaydata: Option<String>,
    ) -> HgCommandRes<BytesOld> {
        let reponame = self.repo.inner_repo().repo_identity().name().to_string();
        cloned!(self.session_bookmarks_cache, self as repoclient);

        let hook_manager = self.repo.hook_manager().clone();

        // Kill the saved set of bookmarks here - the unbundle may change them, and the next
        // command in sequence will need to fetch a new set
        self.session_bookmarks_cache.drop_cache();

        let lfs_params = self.lfs_params();

        let client = repoclient.clone();
        repoclient
            .command_future(ops::UNBUNDLE, UNSAMPLED, move |ctx, command_logger| {
                async move {
                    let repo = client.repo.inner_repo();
                    let lca_hint: Arc<dyn LeastCommonAncestorsHint> = repo.skiplist_index_arc();
                    let infinitepush_params = repo.repo_config().infinitepush.clone();
                    let infinitepush_writes_allowed = infinitepush_params.allow_writes;
                    let pushrebase_params = repo.repo_config().pushrebase.clone();
                    let push_params = repo.repo_config().push.clone();
                    let pure_push_allowed = push_params.pure_push_allowed;
                    let reponame = repo.repo_identity().name().to_string();
                    let maybe_backup_repo_source = client.maybe_backup_repo_source.clone();

                    let pushrebase_flags = pushrebase_params.flags.clone();
                    let action = unbundle::resolve(
                        &ctx,
                        repo.as_blob_repo(),
                        infinitepush_writes_allowed,
                        stream.compat().boxed(),
                        pure_push_allowed,
                        pushrebase_flags,
                        maybe_backup_repo_source,
                    )
                    .await?;

                    let unbundle_future = async {
                        maybe_validate_pushed_bonsais(&ctx, repo.as_blob_repo(), &maybereplaydata)
                            .await?;

                        match client.maybe_get_pushredirector_for_action(&ctx, &action)? {
                            Some(push_redirector) => {
                                // Push-redirection will cause
                                // hooks to be run in the large
                                // repo, but we must also run them
                                // in the small repo.
                                run_hooks(
                                    &ctx,
                                    repo.as_blob_repo(),
                                    hook_manager.as_ref(),
                                    &action,
                                    CrossRepoPushSource::NativeToThisRepo,
                                )
                                .await?;

                                let ctx = ctx.with_mutated_scuba(|mut sample| {
                                    sample.add(
                                        "target_repo_name",
                                        push_redirector.repo.inner_repo().repo_identity().name(),
                                    );
                                    sample.add(
                                        "target_repo_id",
                                        push_redirector.repo.inner_repo().repo_identity().id().id(),
                                    );
                                    sample
                                });
                                ctx.scuba()
                                    .clone()
                                    .log_with_msg("Push redirected to large repo", None);
                                push_redirector
                                    .run_redirected_post_resolve_action(&ctx, action)
                                    .await?
                            }
                            None => {
                                run_post_resolve_action(
                                    &ctx,
                                    repo,
                                    &lca_hint,
                                    &infinitepush_params,
                                    &pushrebase_params,
                                    &push_params,
                                    hook_manager.as_ref(),
                                    action,
                                    CrossRepoPushSource::NativeToThisRepo,
                                )
                                .await?
                            }
                        }
                        .generate_bytes(
                            &ctx,
                            repo.as_blob_repo(),
                            &reponame,
                            pushrebase_params,
                            &lca_hint,
                            &lfs_params,
                            respondlightly,
                        )
                        .await
                    };

                    let response = unbundle_future.await?;

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
                    Ok(response)
                }
                .inspect_err({
                    cloned!(reponame);
                    move |err| {
                        use BundleResolverError::*;
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
                .inspect_ok(move |_| STATS::push_success.add_value(1, (reponame,)))
                .map_ok(bytes_ext::copy_from_new)
                .map_err(Error::from)
                .timeout(default_timeout())
                .flatten_err()
                .timed()
                .map(move |(stats, res)| {
                    command_logger.without_wireproto().finalize_command(&stats);
                    res
                })
                .boxed()
                .compat()
            })
            .boxify()
    }

    // @wireprotocommand('gettreepack', 'rootdir mfnodes basemfnodes directories')
    fn gettreepack(&self, params: GettreepackArgs) -> BoxStream<BytesOld, Error> {
        let sampling_rate = gettreepack_scuba_sampling_rate(&params);
        self.command_stream(
            ops::GETTREEPACK,
            sampling_rate,
            |ctx, mut command_logger| {
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

                log_gettreepack_params_verbose(&ctx, &params);

                let s = self
                    .gettreepack_untimed(ctx.clone(), params)
                    .compat()
                    .whole_stream_timeout(default_timeout())
                    .yield_periodically()
                    .flatten_err()
                    .inspect_ok({
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
                        move |stats| {
                            if stats.completion_time > *SLOW_REQUEST_THRESHOLD {
                                command_logger.add_trimmed_scuba_extra("command_args", &args);
                            }
                            STATS::gettreepack_ms
                                .add_value(stats.completion_time.as_millis_unchecked() as i64);
                            command_logger.finalize_command(&stats);
                            future::ready(())
                        }
                    })
                    .boxed()
                    .compat();

                throttle_stream(
                    &self.session,
                    Metric::TotalManifests,
                    ops::GETTREEPACK,
                    move || s,
                )
            },
        )
    }

    // @wireprotocommand('stream_out_shallow')
    fn stream_out_shallow(&self, tag: Option<String>) -> BoxStream<BytesOld, Error> {
        self.command_stream(ops::STREAMOUTSHALLOW, UNSAMPLED, |ctx, command_logger| {
            let streaming_clone = self.repo.inner_repo().streaming_clone_arc();

            let stream = {
                cloned!(ctx);
                async move {
                    let changelog = streaming_clone
                        .fetch_changelog(ctx.clone(), tag.as_deref())
                        .await?;

                    let data_blobs = changelog
                        .data_blobs
                        .into_iter()
                        .map(|fut| {
                            cloned!(ctx);
                            async move {
                                let (stats, res) = fut.timed().await;
                                ctx.perf_counters().add_to_counter(
                                    PerfCounterType::SumManifoldPollTime,
                                    stats.poll_time.as_nanos_unchecked() as i64,
                                );
                                if let Ok(bytes) = res.as_ref() {
                                    ctx.perf_counters().add_to_counter(
                                        PerfCounterType::BytesSent,
                                        bytes.len() as i64,
                                    )
                                }
                                res
                            }
                            .boxed()
                        })
                        .collect();

                    let index_blobs = changelog
                        .index_blobs
                        .into_iter()
                        .map(|fut| {
                            cloned!(ctx);
                            async move {
                                let (stats, res) = fut.timed().await;
                                ctx.perf_counters().add_to_counter(
                                    PerfCounterType::SumManifoldPollTime,
                                    stats.poll_time.as_nanos_unchecked() as i64,
                                );
                                if let Ok(bytes) = res.as_ref() {
                                    ctx.perf_counters().add_to_counter(
                                        PerfCounterType::BytesSent,
                                        bytes.len() as i64,
                                    )
                                }
                                res
                            }
                            .boxed()
                        })
                        .collect();

                    let changelog = RevlogStreamingChunks {
                        data_size: changelog.data_size,
                        index_size: changelog.index_size,
                        data_blobs,
                        index_blobs,
                    };

                    debug!(
                        ctx.logger(),
                        "streaming changelog {} index bytes, {} data bytes",
                        changelog.index_size,
                        changelog.data_size
                    );

                    let mut response_header = Vec::new();
                    // Send OK response.
                    response_header.push(Bytes::from_static(b"0\n"));
                    // send header.
                    let total_size = changelog.index_size + changelog.data_size;
                    let file_count = 2;
                    let header = format!("{} {}\n", file_count, total_size);
                    response_header.push(header.into_bytes().into());
                    let response = stream::iter(response_header.into_iter().map(Ok));

                    fn build_file_stream(
                        name: &str,
                        size: usize,
                        data: Vec<futures::future::BoxFuture<'static, Result<Bytes, Error>>>,
                    ) -> impl futures::stream::Stream<Item = Result<Bytes, Error>> + Send
                    {
                        let header = format!("{}\0{}\n", name, size);

                        stream::once(future::ready(Ok(header.into_bytes().into())))
                            .chain(stream::iter(data.into_iter()).buffered(100))
                    }

                    let res = response
                        .chain(build_file_stream(
                            "00changelog.i",
                            changelog.index_size,
                            changelog.index_blobs,
                        ))
                        .chain(build_file_stream(
                            "00changelog.d",
                            changelog.data_size,
                            changelog.data_blobs,
                        ));

                    Ok(res)
                }
            }
            .try_flatten_stream();

            stream
                .whole_stream_timeout(clone_timeout())
                .yield_periodically()
                .flatten_err()
                .map_ok(bytes_ext::copy_from_new)
                .timed(|stats| {
                    command_logger.finalize_command(&stats);
                    future::ready(())
                })
                .boxed()
                .compat()
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
        self.command_stream(ops::GETCOMMITDATA, UNSAMPLED, |ctx, mut command_logger| {
            let args = json!(nodes);
            let blobrepo = self.repo.blob_repo().clone();
            ctx.scuba()
                .clone()
                .add("getcommitdata_nodes", nodes.len())
                .log_with_msg("GetCommitData Params", None);

            let s = stream::iter(nodes.into_iter())
                .map({
                    cloned!(ctx, blobrepo);
                    move |hg_cs_id| {
                        cloned!(ctx, blobrepo, hg_cs_id);
                        async move {
                            let revlog_cs =
                                RevlogChangeset::load(&ctx, blobrepo.blobstore(), hg_cs_id).await?;
                            let bytes = serialize_getcommitdata(hg_cs_id, revlog_cs)?;
                            Result::<_, Error>::Ok(bytes)
                        }
                    }
                })
                .buffered(100)
                .inspect_ok({
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
                .whole_stream_timeout(default_timeout())
                .yield_periodically()
                .flatten_err()
                .timed(move |stats| {
                    if stats.completion_time > *SLOW_REQUEST_THRESHOLD {
                        command_logger.add_trimmed_scuba_extra("command_args", &args);
                    }
                    STATS::getcommitdata_ms
                        .add_value(stats.completion_time.as_millis_unchecked() as i64);
                    command_logger.finalize_command(&stats);
                    future::ready(())
                })
                .boxed()
                .compat();

            throttle_stream(
                &self.session,
                Metric::Commits,
                ops::GETCOMMITDATA,
                move || s,
            )
        })
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

    if fetchdepth == Some(1) && !directories.is_empty() {
        if directories.len() != mfnodes.len() {
            let e = format_err!(
                "invalid directories count ({}, expected {})",
                directories.len(),
                mfnodes.len()
            );
            return stream_old::once(Err(e)).boxify();
        }

        if rootdir.is_some() {
            let e = Error::msg("rootdir must be empty");
            return stream_old::once(Err(e)).boxify();
        }

        if !basemfnodes.is_empty() {
            let e = Error::msg("basemfnodes must be empty");
            return stream_old::once(Err(e)).boxify();
        }

        let entries = mfnodes
            .into_iter()
            .zip(directories.into_iter())
            .map(|(node, path)| {
                let path = if !path.is_empty() {
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

        return stream_old::iter_ok::<_, Error>(entries).boxify();
    }

    if !directories.is_empty() {
        // This param is not used by core hg, don't worry about implementing it now
        return stream_old::once(Err(Error::msg("directories param is not supported"))).boxify();
    }

    // 65536 matches the default TREE_DEPTH_MAX value from Mercurial
    let fetchdepth = fetchdepth.unwrap_or(2 << 16);

    // TODO(stash): T25850889 only one basemfnodes is used. That means that trees that client
    // already has can be sent to the client.
    let mut basemfnode = basemfnodes.iter().next().cloned();

    cloned!(repo);
    stream_old::iter_ok::<_, Error>(
        mfnodes
            .into_iter()
            .filter(move |node| !basemfnodes.contains(node))
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
        return stream_old::iter_ok(vec![(mfid, rootpath)]).boxify();
    }

    basemfid
        .filtered_diff(
            ctx,
            repo.get_blobstore(),
            mfid,
            repo.get_blobstore(),
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
        .compat()
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

    let envelope_fut = {
        cloned!(ctx, repo);
        async move { fetch_manifest_envelope(&ctx, repo.blobstore(), hg_mf_id).await }
    }
    .boxed()
    .compat();

    let filenode_fut = {
        cloned!(repo, ctx, repo_path);
        async move {
            repo.get_filenode_opt(ctx, &repo_path, HgFileNodeId::new(hg_mf_id.into_nodehash()))
                .await
        }
    }
    .boxed()
    .compat()
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
    let expected = calculate_hg_node_id(content, parents);

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

            tokio::time::sleep(Duration::from_secs(interval)).await;

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

#[derive(Clone, Debug, PartialEq, Eq)]
enum GitLookup {
    GitToHg(GitSha1),
    HgToGit(HgChangesetId),
}

impl GitLookup {
    pub async fn lookup(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
    ) -> Result<Option<BytesOld>, Error> {
        use GitLookup::*;
        match self {
            GitToHg(git_sha1) => {
                let bonsai_git_mapping = repo.bonsai_git_mapping();

                let maybe_bonsai = bonsai_git_mapping
                    .get_bonsai_from_git_sha1(ctx, *git_sha1)
                    .await?;

                match maybe_bonsai {
                    Some(bcs_id) => {
                        let hg_cs_id = repo.derive_hg_changeset(ctx, bcs_id).await?;
                        Ok(Some(generate_lookup_resp_buf(
                            true,
                            hg_cs_id.to_hex().as_bytes(),
                        )))
                    }
                    None => Ok(None),
                }
            }
            HgToGit(hg_changeset_id) => {
                let maybe_bcs_id = repo
                    .bonsai_hg_mapping()
                    .get_bonsai_from_hg(ctx, *hg_changeset_id)
                    .await?;
                let bonsai_git_mapping = repo.bonsai_git_mapping();

                let bcs_id = match maybe_bcs_id {
                    Some(bcs_id) => bcs_id,
                    None => {
                        return Ok(None);
                    }
                };

                let maybe_git_sha1 = bonsai_git_mapping
                    .get_git_sha1_from_bonsai(ctx, bcs_id)
                    .await?;
                match maybe_git_sha1 {
                    Some(git_sha1) => Ok(Some(generate_lookup_resp_buf(
                        true,
                        git_sha1.to_hex().as_bytes(),
                    ))),
                    None => Ok(None),
                }
            }
        }
    }
}

fn parse_git_lookup(s: &str) -> Option<GitLookup> {
    let hg_prefix = "_gitlookup_hg_";
    let git_prefix = "_gitlookup_git_";

    if let Some(hg_hash) = s.strip_prefix(hg_prefix) {
        Some(GitLookup::HgToGit(HgChangesetId::from_str(hg_hash).ok()?))
    } else if let Some(git_hash) = s.strip_prefix(git_prefix) {
        Some(GitLookup::GitToHg(GitSha1::from_str(git_hash).ok()?))
    } else {
        None
    }
}

fn generate_lookup_resp_buf(success: bool, message: &[u8]) -> BytesOld {
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

#[derive(Debug, Deserialize)]
struct ReplayData {
    hgbonsaimapping: Option<HashMap<HgChangesetId, ChangesetId>>,
}

// Client might send us the bonsai commits it expects to see for given hg changesets.
// This function verifies them.
async fn maybe_validate_pushed_bonsais(
    ctx: &CoreContext,
    repo: &BlobRepo,
    maybereplaydata: &Option<String>,
) -> Result<(), Error> {
    let parsed: ReplayData = match maybereplaydata {
        Some(s) => serde_json::from_str(s.as_str()).context("failed to parse replay data")?,
        None => {
            return Ok(());
        }
    };

    let hgbonsaimapping = match parsed.hgbonsaimapping {
        Some(hgbonsaimapping) => hgbonsaimapping,
        None => {
            return Ok(());
        }
    };

    let hgbonsaimapping: Vec<_> = hgbonsaimapping.into_iter().collect();
    for chunk in hgbonsaimapping.chunks(100) {
        let hg_cs_ids: Vec<_> = chunk.iter().map(|(hg_cs_id, _)| *hg_cs_id).collect();

        let entries = repo.bonsai_hg_mapping().get(ctx, hg_cs_ids.into()).await?;

        let actual_entries = entries
            .into_iter()
            .map(|entry| (entry.hg_cs_id, entry.bcs_id))
            .collect::<HashMap<_, _>>();

        for (hg_cs_id, bcs_id) in chunk {
            match actual_entries.get(hg_cs_id) {
                Some(actual_bcs_id) => {
                    if actual_bcs_id != bcs_id {
                        return Err(format_err!(
                            "Hg changeset {} should map to {}, but it actually maps to {} in {}",
                            hg_cs_id,
                            bcs_id,
                            actual_bcs_id,
                            repo.name(),
                        ));
                    }
                }
                None => {
                    return Err(format_err!(
                        "Hg changeset {} does not exist in {}",
                        hg_cs_id,
                        repo.name(),
                    ));
                }
            };
        }
    }

    Ok(())
}
