/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::future::Future;
use std::hash::Hash;
use std::hash::Hasher;
use std::num::NonZeroU64;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::format_err;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::Bookmark;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkPrefix;
use bookmarks_types::BookmarkKind;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;
use cloned::cloned;
use context::CoreContext;
use context::LoggingContainer;
use context::PerfCounterType;
use context::PerfCounters;
use context::SessionContainer;
use cross_repo_sync::SubmoduleDeps;
use futures::compat::Future01CompatExt;
use futures::future;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream::BoxStream;
use futures::stream::FuturesUnordered;
use futures::stream::TryStreamExt;
use futures_01_ext::FutureExt as OldFutureExt;
use futures_ext::FbFutureExt;
use futures_ext::FbTryFutureExt;
use futures_old::Future as OldFuture;
use futures_old::future as future_old;
use futures_stats::TimedFutureExt;
use getbundle_response::SessionLfsParams;
use hgproto::HgCommandRes;
use hgproto::HgCommands;
use hook_manager::manager::HookManagerArc;
use hostname::get_hostname;
use maplit::hashmap;
use mercurial_bundles::Bundle2Item;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use mercurial_types::HgChangesetIdPrefix;
use mercurial_types::HgChangesetIdsResolvedFromPrefix;
use mercurial_types::percent_encode;
use metaconfig_types::RepoConfigRef;
use mononoke_types::ChangesetId;
use mononoke_types::hash::GitSha1;
use nonzero_ext::nonzero;
use phases::PhasesArc;
use repo_authorization::AuthorizationContext;
use repo_blobstore::RepoBlobstoreRef;
use repo_cross_repo::RepoCrossRepoRef;
use repo_identity::RepoIdentityRef;
use serde::Deserialize;
use serde_json::json;
use stats::prelude::*;
use tracing::debug;
use tracing::info;
use unbundle::BundleResolverError;
use unbundle::CrossRepoPushSource;
use unbundle::PushRedirector;
use unbundle::PushRedirectorArgs;
use unbundle::run_hooks;
use unbundle::run_post_resolve_action;

use crate::Repo;

mod logging;
mod session_bookmarks_cache;
mod tests;

use logging::CommandLogger;
use session_bookmarks_cache::SessionBookmarkCache;

define_stats! {
    prefix = "mononoke.repo_client";
    push_success: dynamic_timeseries("push_success.{}", (reponame: String); Rate, Sum),
    push_hook_failure: dynamic_timeseries("push_hook_failure.{}.{}", (reponame: String, hook_failure: String); Rate, Sum),
    push_conflicts: dynamic_timeseries("push_conflicts.{}", (reponame: String); Rate, Sum),
    rate_limits_exceeded: dynamic_timeseries("rate_limits_exceeded.{}", (reponame: String); Rate, Sum),
    push_error: dynamic_timeseries("push_error.{}", (reponame: String); Rate, Sum),
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
}

#[derive(Clone, Copy, Debug)]
struct SamplingRate(NonZeroU64);

const UNSAMPLED: SamplingRate = SamplingRate(nonzero!(1u64));

fn default_timeout() -> Duration {
    const FALLBACK_TIMEOUT_SECS: u64 = 15 * 60;

    let timeout: u64 = justknobs::get_as::<u64>(
        "scm/mononoke_timeouts:repo_client_default_timeout_secs",
        None,
    )
    .unwrap_or(FALLBACK_TIMEOUT_SECS);

    Duration::from_secs(timeout)
}

fn wireprotocaps() -> Vec<String> {
    vec![
        "clienttelemetry".to_string(),
        "lookup".to_string(),
        "known".to_string(),
        "unbundle=HG10GZ,HG10BZ,HG10UN".to_string(),
        "unbundlereplay".to_string(),
        "remotefilelog".to_string(),
        "pushkey".to_string(),
    ]
}

fn bundle2caps() -> String {
    let caps = vec![
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
        ("b2x:infinitepushmutation", vec![]),
    ];

    let mut encodedcaps = vec![];

    for (key, value) in &caps {
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

#[derive(Clone)]
pub struct RepoClient<R: Repo> {
    repo: Arc<R>,
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
    session_bookmarks_cache: Arc<SessionBookmarkCache<Arc<R>>>,
    maybe_push_redirector_args: Option<PushRedirectorArgs<R>>,
    force_lfs: Arc<AtomicBool>,

    request_perf_counters: Arc<PerfCounters>,
}

impl<R: Repo> RepoClient<R> {
    pub fn new(
        repo: Arc<R>,
        session: SessionContainer,
        logging: LoggingContainer,
        maybe_push_redirector_args: Option<PushRedirectorArgs<R>>,
    ) -> Self {
        let session_bookmarks_cache = Arc::new(SessionBookmarkCache::new(repo.clone()));

        Self {
            repo,
            session,
            logging,
            session_bookmarks_cache,
            maybe_push_redirector_args,
            force_lfs: Arc::new(AtomicBool::new(false)),
            request_perf_counters: Arc::new(PerfCounters::default()),
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
    ) -> BoxFuture<'static, Result<I, E>>
    where
        F: Future<Output = Result<I, E>> + Send + 'static,
        H: FnOnce(CoreContext, CommandLogger) -> F,
    {
        let (ctx, command_logger) = self.start_command(command, sampling_rate);
        Box::pin(handler(ctx, command_logger))
    }

    fn start_command(
        &self,
        command: &str,
        sampling_rate: SamplingRate,
    ) -> (CoreContext, CommandLogger) {
        match command {
            "hello" | "clienttelemetry" => debug!("{}", command),
            _ => info!("{}", command),
        }

        let mut scuba = self.logging.scuba().clone();
        scuba
            .sampled_unless_verbose(sampling_rate.0)
            .add("command", command);
        scuba.clone().log_with_msg("Start processing", None);

        let ctx = self
            .session
            .new_context_with_scribe(scuba, self.logging.scribe().clone());

        let command_logger = CommandLogger::new(ctx.clone(), self.request_perf_counters.clone());

        (ctx, command_logger)
    }

    fn get_publishing_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Output = Result<HashMap<Bookmark, HgChangesetId>>> + use<R> {
        let session_bookmarks_cache = self.session_bookmarks_cache.clone();
        async move { session_bookmarks_cache.get_publishing_bookmarks(ctx).await }
    }

    fn get_pull_default_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Output = Result<HashMap<Vec<u8>, Vec<u8>>>> + use<R> {
        let publishing_bookmarks = self.get_publishing_bookmarks_maybe_stale(ctx);

        async move {
            Ok(publishing_bookmarks
                .await?
                .into_iter()
                .filter_map(|(book, cs)| {
                    let hash: Vec<u8> = cs.into_nodehash().to_hex().into();
                    if book.kind() == &BookmarkKind::PullDefaultPublishing {
                        Some((book.into_key().into_byte_vec(), hash))
                    } else {
                        None
                    }
                })
                .collect())
        }
    }

    fn lfs_params(&self) -> SessionLfsParams {
        if self.force_lfs.load(Ordering::Relaxed) {
            SessionLfsParams {
                threshold: self.repo.repo_config().lfs.threshold,
            }
        } else {
            let client_hostname = self.session.metadata().client_hostname();
            let percentage = self.repo.repo_config().lfs.rollout_percentage;

            let allowed = match client_hostname {
                Some(client_hostname) => {
                    let mut hasher = DefaultHasher::new();
                    client_hostname.hash(&mut hasher);
                    hasher.finish() % 100 < u64::from(percentage)
                }
                None => {
                    // Randomize in case source hostname is not set to avoid
                    // sudden jumps in traffic
                    rand::random_ratio(percentage, 100)
                }
            };

            if allowed {
                SessionLfsParams {
                    threshold: self.repo.repo_config().lfs.threshold,
                }
            } else {
                SessionLfsParams { threshold: None }
            }
        }
    }

    async fn maybe_get_pushredirector_for_action(
        &self,
        ctx: &CoreContext,
        action: &unbundle::PostResolveAction,
    ) -> Result<Option<PushRedirector<R>>> {
        let push_redirector_args = match self.maybe_push_redirector_args.clone() {
            Some(push_redirector_args) => push_redirector_args,
            None => {
                debug!("maybe_push_redirector_args are none, no push_redirector for unbundle");
                return Ok(None);
            }
        };

        use unbundle::PostResolveAction::*;

        let live_commit_sync_config = self
            .repo
            .repo_cross_repo()
            .live_commit_sync_config()
            .clone();

        let repo_id = self.repo.repo_identity().id();
        let redirect = match action {
            InfinitePush(_) => {
                live_commit_sync_config
                    .push_redirector_enabled_for_draft(ctx, repo_id)
                    .await?
            }
            Push(_) | PushRebase(_) | BookmarkOnlyPushRebase(_) => {
                live_commit_sync_config
                    .push_redirector_enabled_for_public(ctx, repo_id)
                    .await?
            }
        };

        if redirect {
            debug!("live_commit_sync_config says push redirection is on");
            Ok(Some(push_redirector_args.into_push_redirector(
                ctx,
                live_commit_sync_config,
                SubmoduleDeps::NotNeeded,
            )?))
        } else {
            debug!("live_commit_sync_config says push redirection is off");
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
        Fut: Future<Output = Result<Vec<bool>, Error>> + Send + 'static,
    {
        self.command_future(command, UNSAMPLED, |ctx, mut command_logger| {
            let repo = self.repo.clone();

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
                    let max_nodes = justknobs::get_as::<usize>(
                        "scm/mononoke:repo_client_max_nodes_in_known_method",
                        None,
                    )?;

                    if max_nodes > 0 {
                        if nodes_len > max_nodes {
                            return Err(format_err!(
                                "invalid request - too many requests were sent in 'known' method"
                            ));
                        }
                    }
                    let hg_bcs_mapping = repo
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
        })
    }
}

impl<R: Repo> HgCommands for RepoClient<R> {
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

                future::ok(hostname).timed().map(move |(stats, res)| {
                    command_logger.without_wireproto().finalize_command(&stats);
                    res
                })
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
                .map_ok(|map| map.into_values().collect())
                .timeout(default_timeout())
                .flatten_err()
                .timed()
                .map(move |(stats, res)| {
                    command_logger.without_wireproto().finalize_command(&stats);
                    res
                })
        })
    }

    // @wireprotocommand('lookup', 'key')
    fn lookup(&self, key: String) -> HgCommandRes<Bytes> {
        // Generate positive response including HgChangesetId as hex.
        fn generate_changeset_resp_buf(csid: HgChangesetId) -> HgCommandRes<Bytes> {
            async move { Ok(generate_lookup_resp_buf(true, csid.to_hex().as_bytes())) }.boxed()
        }

        // Generate error response with the message including suggestions (commits info).
        // Suggestions are ordered by commit time (most recent first).
        fn generate_suggestions_resp_buf<R: Repo>(
            ctx: CoreContext,
            repo: R,
            suggestion_cids: Vec<HgChangesetId>,
        ) -> HgCommandRes<Bytes> {
            let futs = suggestion_cids
                .into_iter()
                .map(|hg_csid| {
                    cloned!(ctx, repo);
                    async move { hg_csid.load(&ctx, repo.repo_blobstore()).await }
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
                .compat()
                .boxed()
        }

        // Controls how many suggestions to fetch in case of ambiguous outcome of prefix lookup.
        const MAX_NUMBER_OF_SUGGESTIONS_TO_FETCH: usize = 10;

        let maybe_git_lookup = parse_git_lookup(&key);
        self.command_future(ops::LOOKUP, UNSAMPLED, |ctx, command_logger| {
            let repo = self.repo.clone();

            // Resolves changeset or set of suggestions from the key (full hex hash or a prefix) if exist.
            // Note: `get_many_hg_by_prefix` works for the full hex hashes well but
            //       `changeset_exists` has better caching and is preferable for the full length hex hashes.
            let node_fut = match HgChangesetId::from_str(&key) {
                Ok(csid) => {
                    cloned!(ctx, repo);
                    async move {
                        if repo.hg_changeset_exists(ctx, csid).await? {
                            Ok(HgChangesetIdsResolvedFromPrefix::Single(csid))
                        } else {
                            Ok(HgChangesetIdsResolvedFromPrefix::NoMatch)
                        }
                    }
                    .boxed()
                }
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
                    }
                    Err(_) => {
                        futures::future::ok(HgChangesetIdsResolvedFromPrefix::NoMatch).boxed()
                    }
                },
            };

            // The lookup order:
            // If there is a git_lookup match, return that.
            // If there is an exact commit match, return that even if the key is the prefix of the hash.
            // If there is a bookmark match, return that.
            // If there are suggestions, show them. This happens in case of ambiguous outcome of prefix lookup.
            // Otherwise, show an error.

            let bookmark = BookmarkKey::new(&key).ok();
            let lookup_fut = node_fut
                .and_then({
                    cloned!(ctx, repo);
                    move |resolved_cids| {
                        use HgChangesetIdsResolvedFromPrefix::*;

                        // Describing the priority relative to bookmark presence for the key.
                        enum LookupOutcome {
                            HighPriority(HgCommandRes<Bytes>),
                            LowPriority(HgCommandRes<Bytes>),
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
                                async move {
                                    Ok(generate_lookup_resp_buf(
                                        false,
                                        format!("ambiguous identifier '{}'", key).as_bytes(),
                                    ))
                                }
                                .boxed(),
                            ),
                            NoMatch => LookupOutcome::LowPriority(
                                async move {
                                    Ok(generate_lookup_resp_buf(
                                        false,
                                        format!("{} not found", key).as_bytes(),
                                    ))
                                }
                                .boxed(),
                            ),
                        };

                        match (outcome, bookmark) {
                            (LookupOutcome::HighPriority(res), _) => res,
                            (LookupOutcome::LowPriority(res), Some(bookmark)) => async move {
                                let maybe_cs_id =
                                    repo.get_bookmark_hg(ctx.clone(), &bookmark).await?;
                                if let Some(csid) = maybe_cs_id {
                                    generate_changeset_resp_buf(csid).await
                                } else {
                                    res.await
                                }
                            }
                            .boxed(),
                            (LookupOutcome::LowPriority(res), None) => res,
                        }
                    }
                })
                .boxed();

            async move {
                if let Some(git_lookup) = maybe_git_lookup {
                    if let Some(res) = git_lookup.lookup(&ctx, &repo).await? {
                        return Ok(res);
                    }
                }
                lookup_fut.await
            }
            .timeout(default_timeout())
            .flatten_err()
            .timed()
            .map(move |(stats, res)| {
                command_logger.without_wireproto().finalize_command(&stats);
                res
            })
        })
    }

    // @wireprotocommand('known', 'nodes *'), but the '*' is ignored
    fn known(&self, nodes: Vec<HgChangesetId>) -> HgCommandRes<Vec<bool>> {
        let phases_hint = self.repo.phases_arc();
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
        })
    }

    // @wireprotocommand('listkeys', 'namespace')
    fn listkeys(&self, namespace: String) -> HgCommandRes<HashMap<Vec<u8>, Vec<u8>>> {
        if namespace == "bookmarks" {
            self.command_future(ops::LISTKEYS, UNSAMPLED, |ctx, command_logger| {
                self.get_pull_default_bookmarks_maybe_stale(ctx)
                    .timed()
                    .map(move |(stats, res)| {
                        command_logger.without_wireproto().finalize_command(&stats);
                        res
                    })
            })
        } else {
            info!("unsupported listkeys namespace: {}", namespace);
            future::ok(HashMap::new()).boxed()
        }
    }

    // @wireprotocommand('listkeyspatterns', 'namespace', 'patterns *')
    fn listkeyspatterns(
        &self,
        namespace: String,
        patterns: Vec<String>,
    ) -> HgCommandRes<BTreeMap<String, HgChangesetId>> {
        if namespace != "bookmarks" {
            info!("unsupported listkeyspatterns namespace: {}", namespace,);
            return future::err(format_err!(
                "unsupported listkeyspatterns namespace: {}",
                namespace
            ))
            .boxed();
        }

        self.command_future(ops::LISTKEYSPATTERNS, UNSAMPLED, |ctx, command_logger| {
            let max = self.repo.repo_config().list_keys_patterns_max;
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
                        let bookmark = BookmarkKey::new(&pattern)?;

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
        })
    }

    // @wireprotocommand('unbundle')
    fn unbundle(
        &self,
        _heads: Vec<String>,
        stream: BoxStream<'static, Result<Bundle2Item<'static>, Error>>,
        respondlightly: Option<bool>,
        maybereplaydata: Option<String>,
    ) -> HgCommandRes<Bytes> {
        let reponame = self.repo.repo_identity().name().to_string();
        cloned!(self.session_bookmarks_cache, self as repoclient);

        let hook_manager = self.repo.hook_manager_arc();

        // Kill the saved set of bookmarks here - the unbundle may change them, and the next
        // command in sequence will need to fetch a new set
        self.session_bookmarks_cache.drop_cache();

        let lfs_params = self.lfs_params();

        let client = repoclient.clone();
        repoclient.command_future(ops::UNBUNDLE, UNSAMPLED, move |ctx, command_logger| {
            async move {
                let repo = &client.repo;

                // To use unbundle wireproto command the user needs at least all-repo `draft` permission.
                // This is overkill - we could check more granular permissions but wireproto is deprecated and
                // it doesn't seem worth auditing each codepath there so let's use the big hammer!
                let authz = AuthorizationContext::new(&ctx);
                authz
                    .require_full_repo_draft(&ctx, repo)
                    .await
                    .map_err(|err| BundleResolverError::Error(err.into()))?;

                let infinitepush_writes_allowed = repo.repo_config().infinitepush.allow_writes;
                let pushrebase_params = repo.repo_config().pushrebase.clone();

                let pushrebase_flags = pushrebase_params.flags.clone();
                let action = unbundle::resolve(
                    &ctx,
                    repo,
                    infinitepush_writes_allowed,
                    stream,
                    &repo.repo_config().push,
                    pushrebase_flags,
                )
                .await?;

                let unbundle_future = async {
                    maybe_validate_pushed_bonsais(&ctx, repo, &maybereplaydata).await?;

                    match client
                        .maybe_get_pushredirector_for_action(&ctx, &action)
                        .await?
                    {
                        Some(push_redirector) => {
                            // Push-redirection will cause
                            // hooks to be run in the large
                            // repo, but we must also run them
                            // in the small repo.
                            run_hooks(
                                &ctx,
                                repo,
                                hook_manager.as_ref(),
                                &action,
                                CrossRepoPushSource::NativeToThisRepo,
                            )
                            .await?;

                            let ctx = ctx.with_mutated_scuba(|mut sample| {
                                sample.add(
                                    "target_repo_name",
                                    push_redirector.repo.repo_identity().name(),
                                );
                                sample.add(
                                    "target_repo_id",
                                    push_redirector.repo.repo_identity().id().id(),
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
                                hook_manager.as_ref(),
                                action,
                                CrossRepoPushSource::NativeToThisRepo,
                            )
                            .await?
                        }
                    }
                    .generate_bytes(&ctx, repo, pushrebase_params, &lfs_params, respondlightly)
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
                        Error(..) => {
                            STATS::push_error.add_value(1, (reponame,));
                        }
                    };
                }
            })
            .inspect_ok(move |_| STATS::push_success.add_value(1, (reponame,)))
            .map_err(Error::from)
            .timeout(default_timeout())
            .flatten_err()
            .timed()
            .map(move |(stats, res)| {
                command_logger.without_wireproto().finalize_command(&stats);
                res
            })
        })
    }
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
        repo: &impl Repo,
    ) -> Result<Option<Bytes>, Error> {
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

fn generate_lookup_resp_buf(success: bool, message: &[u8]) -> Bytes {
    let mut buf = BytesMut::with_capacity(message.len() + 3);
    if success {
        buf.put_u8(b'1');
    } else {
        buf.put_u8(b'0');
    }
    buf.put_u8(b' ');
    buf.put(message);
    buf.put_u8(b'\n');
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
    repo: &impl Repo,
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
                            repo.repo_identity().name(),
                        ));
                    }
                }
                None => {
                    return Err(format_err!(
                        "Hg changeset {} does not exist in {}",
                        hg_cs_id,
                        repo.repo_identity().name(),
                    ));
                }
            };
        }
    }

    Ok(())
}
