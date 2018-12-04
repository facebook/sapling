// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

mod remotefilelog;
pub mod streaming_clone;

use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;
use std::mem;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use bytes::{BufMut, Bytes, BytesMut};
use failure::err_msg;
use futures::{future, stream, Async, Future, IntoFuture, Poll, Stream, stream::empty};
use futures_ext::{select_all, BoxFuture, BoxStream, FutureExt, StreamExt};
use futures_stats::{StreamStats, Timed, TimedStreamTrait};
use itertools::Itertools;
use slog::Logger;
use stats::Histogram;
use time_ext::DurationExt;

use blobrepo::HgBlobChangeset;
use bookmarks::Bookmark;
use bundle2_resolver;
use context::CoreContext;
use mercurial_bundles::{create_bundle_stream, parts, Bundle2Item};
use mercurial_types::{percent_encode, Entry, HgBlobNode, HgChangesetId, HgManifestId, HgNodeHash,
                      MPath, RepoPath, Type, NULL_HASH};
use mercurial_types::manifest_utils::{changed_entry_stream_with_pruner, CombinatorPruner,
                                      DeletedPruner, EntryStatus, FilePruner, Pruner,
                                      VisitedPruner};
use rand;
use reachabilityindex::LeastCommonAncestorsHint;
use scribe::ScribeClient;
use scuba_ext::{ScribeClientImplementation, ScubaSampleBuilder, ScubaSampleBuilderExt};
use serde_json;
use tracing::{TraceContext, Traced};

use blobrepo::BlobRepo;
use hgproto::{self, GetbundleArgs, GettreepackArgs, HgCommandRes, HgCommands};

use self::remotefilelog::create_remotefilelog_blob;
use self::streaming_clone::RevlogStreamingChunks;

use errors::*;
use hooks::HookManager;
use metaconfig::repoconfig::RepoReadOnly;
use mononoke_repo::{MononokeRepo, SqlStreamingCloneConfig};

const MAX_NODES_TO_LOG: usize = 5;

define_stats! {
    prefix = "mononoke.repo_client";
    getbundle_ms:
        histogram(500, 0, 10_000, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    gettreepack_ms:
        histogram(500, 0, 20_000, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    getfiles_ms:
        histogram(500, 0, 20_000, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
}

mod ops {
    pub static HELLO: &str = "hello";
    pub static UNBUNDLE: &str = "unbundle";
    pub static HEADS: &str = "heads";
    pub static LOOKUP: &str = "lookup";
    pub static LISTKEYS: &str = "listkeys";
    pub static KNOWN: &str = "known";
    pub static BETWEEN: &str = "between";
    pub static GETBUNDLE: &str = "getbundle";
    pub static GETTREEPACK: &str = "gettreepack";
    pub static GETFILES: &str = "getfiles";
}

fn format_nodes_list(nodes: &Vec<HgNodeHash>) -> String {
    nodes.iter().map(|node| format!("{}", node)).join(" ")
}

fn format_utf8_bytes_list<T: AsRef<[u8]>>(entries: &Vec<T>) -> String {
    entries
        .iter()
        .map(|entry| String::from_utf8_lossy(entry.as_ref()).into_owned())
        .join(",")
}

fn wireprotocaps() -> Vec<String> {
    vec![
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
    ]
}

fn bundle2caps() -> String {
    let caps = vec![
        ("HG20", vec![]),
        // Note that "listkeys" is *NOT* returned as a bundle2 capability; that's because there's
        // a race that can happen. Here's how:
        // 1. The client does discovery to figure out which heads are missing.
        // 2. At this point, a frequently updated bookmark (say "master") moves forward.
        // 3. The client requests the heads discovered in step 1 + the latest value of master.
        // 4. The server returns changesets up to those heads, plus the latest version of master.
        //
        // master doesn't point to a commit that will exist on the client at the end of the pull,
        // so the client ignores it.
        //
        // The workaround here is to force bookmarks to be sent before discovery happens. Disabling
        // the listkeys capabilities causes the Mercurial client to do that.
        //
        // A better fix might be to snapshot and maintain the bookmark state on the server at the
        // start of discovery.
        //
        // The best fix here would be to change the protocol to represent bookmark pulls
        // atomically.
        //
        // Some other notes:
        // * Stock Mercurial doesn't appear to have this problem. @rain1 hasn't verified why, but
        //   believes it's because bookmarks get loaded up into memory before discovery and then
        //   don't get reloaded for the duration of the process. (In Mononoke, this is the
        //   "snapshot and maintain the bookmark state" approach mentioned above.)
        // * There's no similar race with pushes updating bookmarks, so "pushkey" is still sent
        //   as a capability.
        // * To repro the race, run test-bookmark-race.t with the following line enabled.

        // ("listkeys", vec![]),
        ("changegroup", vec!["02"]),
        ("b2x:infinitepush", vec![]),
        ("b2x:infinitepushscratchbookmarks", vec![]),
        ("pushkey", vec![]),
        ("treemanifestserver", vec!["True"]),
        ("b2x:rebase", vec![]),
        ("b2x:rebasepackpart", vec![]),
    ];

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
    lca_hint: Arc<LeastCommonAncestorsHint + Send + Sync>,
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

        logger.args = args;
        if let Some(ref args) = logger.args {
            let args = args.to_string();
            // Scuba does not support too long columns, we have to trim it
            let limit = ::std::cmp::min(args.len(), 1000);
            logger.scuba_logger.add("command_args", &args[..limit]);
        }

        logger.scuba_logger.log_with_msg("Start processing", None);
        logger
    }

    fn set_args(&mut self, args: Option<serde_json::Value>) {
        self.args = args;
    }

    fn finish_stream_wireproto_processing(&mut self, stats: &StreamStats) {
        self.scuba_logger
            .add_stream_stats(&stats)
            .log_with_msg("Command processed", None);

        if let Some(ref wireproto_scribe_category) = self.wireproto_scribe_category {
            let mut builder = ScubaSampleBuilder::with_discard();
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
            builder.add("reponame", self.reponame.as_str());

            // We can't really do anything with the errors, so let's ignore it
            let sample = builder.get_sample();
            if let Ok(sample_json) = sample.to_json() {
                let _ = self.scribe_client
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
        lca_hint: Arc<LeastCommonAncestorsHint + Send + Sync>,
    ) -> Self {
        RepoClient {
            repo,
            ctx,
            hash_validation_percentage,
            lca_hint,
        }
    }

    fn logger(&self) -> &Logger {
        self.ctx.logger()
    }

    fn trace(&self) -> &TraceContext {
        self.ctx.trace()
    }

    fn scuba_logger(&self, op: &str, args: Option<String>) -> ScubaSampleBuilder {
        let mut scuba_logger = self.ctx.scuba().clone();

        scuba_logger.add("command", op);

        if let Some(args) = args {
            scuba_logger.add("command_args", args);
        }

        scuba_logger.log_with_msg("Start processing", None);
        scuba_logger
    }

    fn create_bundle(&self, args: GetbundleArgs) -> Result<BoxStream<Bytes, Error>> {
        let blobrepo = self.repo.blobrepo();
        let mut bundle2_parts = vec![];
        let cg_part_builder = bundle2_resolver::create_getbundle_response(
            self.ctx.clone(),
            blobrepo.clone(),
            args.common
                .into_iter()
                .map(|head| HgChangesetId::new(head))
                .collect(),
            args.heads
                .into_iter()
                .map(|head| HgChangesetId::new(head))
                .collect(),
            self.lca_hint.clone(),
        )?;
        bundle2_parts.push(cg_part_builder);

        // XXX Note that listkeys is NOT returned as a bundle2 capability -- see comment in
        // bundle2caps() for why.

        // TODO: generalize this to other listkey types
        // (note: just calling &b"bookmarks"[..] doesn't work because https://fburl.com/0p0sq6kp)
        if args.listkeys.contains(&b"bookmarks".to_vec()) {
            let items = blobrepo
                .get_bookmarks_maybe_stale(self.ctx.clone())
                .map(|(name, cs)| {
                    let hash: Vec<u8> = cs.into_nodehash().to_hex().into();
                    (name.to_string(), hash)
                });
            bundle2_parts.push(parts::listkey_part("bookmarks", items)?);
        }
        // TODO(stash): handle includepattern= and excludepattern=

        let compression = None;
        Ok(create_bundle_stream(bundle2_parts, compression).boxify())
    }

    fn gettreepack_untimed(&self, params: GettreepackArgs) -> BoxStream<Bytes, Error> {
        debug!(self.logger(), "gettreepack");

        // 65536 matches the default TREE_DEPTH_MAX value from Mercurial
        let fetchdepth = params.depth.unwrap_or(2 << 16);

        if !params.directories.is_empty() {
            // This param is not used by core hg, don't worry about implementing it now
            return stream::once(Err(err_msg("directories param is not supported"))).boxify();
        }

        // TODO(stash): T25850889 only one basemfnodes is used. That means that trees that client
        // already has can be sent to the client.
        let basemfnode = params.basemfnodes.get(0).cloned().unwrap_or(NULL_HASH);

        let rootpath = if params.rootdir.is_empty() {
            None
        } else {
            Some(try_boxstream!(MPath::new(params.rootdir)))
        };

        let default_pruner = CombinatorPruner::new(FilePruner, DeletedPruner);

        let changed_entries = if params.mfnodes.len() > 1 {
            let visited_pruner = VisitedPruner::new();
            select_all(params.mfnodes.iter().map(|manifest_id| {
                get_changed_manifests_stream(
                    self.repo.blobrepo(),
                    &manifest_id,
                    &basemfnode,
                    rootpath.clone(),
                    CombinatorPruner::new(default_pruner.clone(), visited_pruner.clone()),
                    fetchdepth,
                    self.trace().clone(),
                )
            })).boxify()
        } else {
            match params.mfnodes.get(0) {
                Some(mfnode) => get_changed_manifests_stream(
                    self.repo.blobrepo(),
                    &mfnode,
                    &basemfnode,
                    rootpath.clone(),
                    default_pruner,
                    fetchdepth,
                    self.trace().clone(),
                ),
                None => empty().boxify(),
            }
        };

        let validate_hash = rand::random::<usize>() % 100 < self.hash_validation_percentage;
        let changed_entries = changed_entries
            .filter({
                let mut used_hashes = HashSet::new();
                move |entry| used_hashes.insert(*entry.0.get_hash())
            })
            .map({
                cloned!(self.ctx);
                let blobrepo = self.repo.blobrepo().clone();
                let trace = self.trace().clone();
                let scuba_logger = self.scuba_logger(ops::GETTREEPACK, None);
                move |(entry, basepath)| {
                    fetch_treepack_part_input(
                        ctx.clone(),
                        &blobrepo,
                        entry,
                        basepath,
                        trace.clone(),
                        validate_hash,
                        scuba_logger.clone(),
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
    fn between(&self, pairs: Vec<(HgNodeHash, HgNodeHash)>) -> HgCommandRes<Vec<Vec<HgNodeHash>>> {
        info!(self.logger(), "between pairs {:?}", pairs);

        struct ParentStream<CS> {
            repo: MononokeRepo,
            n: HgNodeHash,
            bottom: HgNodeHash,
            wait_cs: Option<CS>,
        };

        impl<CS> ParentStream<CS> {
            fn new(repo: &MononokeRepo, top: HgNodeHash, bottom: HgNodeHash) -> Self {
                ParentStream {
                    repo: repo.clone(),
                    n: top,
                    bottom: bottom,
                    wait_cs: None,
                }
            }
        }

        impl Stream for ParentStream<BoxFuture<HgBlobChangeset, hgproto::Error>> {
            type Item = HgNodeHash;
            type Error = hgproto::Error;

            fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
                if self.n == self.bottom || self.n == NULL_HASH {
                    return Ok(Async::Ready(None));
                }

                self.wait_cs = self.wait_cs.take().or_else(|| {
                    Some(
                        self.repo
                            .blobrepo()
                            .get_changeset_by_changesetid(&HgChangesetId::new(self.n)),
                    )
                });
                let cs = try_ready!(self.wait_cs.as_mut().unwrap().poll());
                self.wait_cs = None; // got it

                let p = cs.p1().cloned().unwrap_or(NULL_HASH);
                let prev_n = mem::replace(&mut self.n, p);

                Ok(Async::Ready(Some(prev_n)))
            }
        }

        let mut scuba_logger = self.scuba_logger(ops::BETWEEN, None);

        // TODO(jsgf): do pairs in parallel?
        // TODO: directly return stream of streams
        let repo = self.repo.clone();
        stream::iter_ok(pairs.into_iter())
            .and_then(move |(top, bottom)| {
                let mut f = 1;
                ParentStream::new(&repo, top, bottom)
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
            .traced(self.trace(), ops::BETWEEN, trace_args!())
            .timed(move |stats, _| {
                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Command processed", None);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('heads')
    fn heads(&self) -> HgCommandRes<HashSet<HgNodeHash>> {
        // Get a stream of heads and collect them into a HashSet
        // TODO: directly return stream of heads
        info!(self.logger(), "heads");
        let mut scuba_logger = self.scuba_logger(ops::HEADS, None);

        self.repo
            .blobrepo()
            .get_heads_maybe_stale(self.ctx.clone())
            .collect()
            .map(|v| v.into_iter().collect())
            .from_err()
            .traced(self.trace(), ops::HEADS, trace_args!())
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
        info!(self.logger(), "lookup: {:?}", key);
        // TODO(stash): T25928839 lookup should support prefixes
        let repo = self.repo.blobrepo().clone();
        let mut scuba_logger = self.scuba_logger(ops::LOOKUP, None);

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
            bookmark: Bookmark,
        ) -> HgCommandRes<Bytes> {
            repo.get_bookmark(ctx, &bookmark)
                .map(move |csid| match csid {
                    Some(csid) => generate_resp_buf(true, csid.to_hex().as_bytes()),
                    None => generate_resp_buf(false, format!("{} not found", bookmark).as_bytes()),
                })
                .boxify()
        }

        let node = HgNodeHash::from_str(&key).ok();
        let bookmark = Bookmark::new(&key).ok();

        let lookup_fut = match (node, bookmark) {
            (Some(node), Some(bookmark)) => {
                let csid = HgChangesetId::new(node);
                repo.changeset_exists(self.ctx.clone(), &csid)
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
            .traced(self.trace(), ops::LOOKUP, trace_args!())
            .timed(move |stats, _| {
                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Command processed", None);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('known', 'nodes *'), but the '*' is ignored
    fn known(&self, nodes: Vec<HgNodeHash>) -> HgCommandRes<Vec<bool>> {
        if nodes.len() > MAX_NODES_TO_LOG {
            info!(self.logger(), "known: {:?}...", &nodes[..MAX_NODES_TO_LOG]);
        } else {
            info!(self.logger(), "known: {:?}", nodes);
        }
        let blobrepo = self.repo.blobrepo().clone();

        let mut scuba_logger = self.scuba_logger(ops::KNOWN, None);

        let nodes: Vec<_> = nodes.into_iter().map(HgChangesetId::new).collect();
        let nodes_len = nodes.len();

        ({
            let ref_nodes: Vec<_> = nodes.iter().collect();
            blobrepo.many_changesets_exists(self.ctx.clone(), &ref_nodes[..])
        }).map(move |cs| {
            let cs: HashSet<_> = cs.into_iter().collect();
            let known_nodes: Vec<_> = nodes
                .into_iter()
                .map(move |node| cs.contains(&node))
                .collect();
            known_nodes
        })
            .traced(self.trace(), ops::KNOWN, trace_args!())
            .timed(move |stats, known_nodes| {
                if let Ok(known) = known_nodes {
                    let extra_context = json!({
                        "num_known": known.len(),
                        "num_unknown": nodes_len - known.len(),
                    }).to_string();

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
        info!(self.logger(), "Getbundle: {:?}", args);

        let value = json!({
            "bundlecaps": format_utf8_bytes_list(&args.bundlecaps),
            "common": format_nodes_list(&args.common),
            "heads": format_nodes_list(&args.heads),
            "listkeys": format_utf8_bytes_list(&args.listkeys),
        });
        let value = json!(vec![value]);
        let mut wireproto_logger = self.wireproto_logger(ops::GETBUNDLE, Some(value));

        match self.create_bundle(args) {
            Ok(res) => res.boxify(),
            Err(err) => stream::once(Err(err)).boxify(),
        }.traced(self.trace(), ops::GETBUNDLE, trace_args!())
            .timed(move |stats, _| {
                STATS::getbundle_ms.add_value(stats.completion_time.as_millis_unchecked() as i64);
                wireproto_logger.finish_stream_wireproto_processing(&stats);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('hello')
    fn hello(&self) -> HgCommandRes<HashMap<String, Vec<String>>> {
        info!(self.logger(), "Hello -> capabilities");

        let mut res = HashMap::new();
        let mut caps = wireprotocaps();
        caps.push(format!("bundle2={}", bundle2caps()));
        res.insert("capabilities".to_string(), caps);

        let mut scuba_logger = self.scuba_logger(ops::HELLO, None);

        future::ok(res)
            .traced(self.trace(), ops::HELLO, trace_args!())
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
        info!(self.logger(), "listkeys: {}", namespace);
        if namespace == "bookmarks" {
            let mut scuba_logger = self.scuba_logger(ops::LISTKEYS, None);

            self.repo
                .blobrepo()
                .get_bookmarks_maybe_stale(self.ctx.clone())
                .map(|(name, cs)| {
                    let hash: Vec<u8> = cs.into_nodehash().to_hex().into();
                    (name, hash)
                })
                .collect()
                .map(|bookmarks| {
                    let bookiter = bookmarks
                        .into_iter()
                        .map(|(name, value)| (Vec::from(name.to_string()), value));
                    HashMap::from_iter(bookiter)
                })
                .traced(self.trace(), ops::LISTKEYS, trace_args!())
                .timed(move |stats, _| {
                    scuba_logger
                        .add_future_stats(&stats)
                        .log_with_msg("Command processed", None);
                    Ok(())
                })
                .boxify()
        } else {
            info!(
                self.logger(),
                "unsupported listkeys namespace: {}",
                namespace
            );
            future::ok(HashMap::new()).boxify()
        }
    }

    // @wireprotocommand('unbundle')
    fn unbundle(
        &self,
        heads: Vec<String>,
        stream: BoxStream<Bundle2Item, Error>,
        hook_manager: Arc<HookManager>,
    ) -> HgCommandRes<Bytes> {
        if self.repo.readonly() == RepoReadOnly::ReadOnly {
            return future::err(ErrorKind::RepoReadOnly.into()).boxify();
        }

        let mut scuba_logger = self.scuba_logger(ops::UNBUNDLE, None);

        let res = bundle2_resolver::resolve(
            self.ctx.clone(),
            Arc::new(self.repo.blobrepo().clone()),
            self.logger().new(o!("command" => "unbundle")),
            scuba_logger.clone(),
            self.repo.pushrebase_params().clone(),
            heads,
            stream,
            hook_manager,
            self.lca_hint.clone(),
        );

        res.traced(self.trace(), ops::UNBUNDLE, trace_args!())
            .timed(move |stats, _| {
                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Command processed", None);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('gettreepack', 'rootdir mfnodes basemfnodes directories')
    fn gettreepack(&self, params: GettreepackArgs) -> BoxStream<Bytes, Error> {
        let args = json!({
            "rootdir": String::from_utf8_lossy(&params.rootdir),
            "mfnodes": format_nodes_list(&params.mfnodes),
            "basemfnodes": format_nodes_list(&params.basemfnodes),
            "directories": format_utf8_bytes_list(&params.directories),
        });
        let args = json!(vec![args]);
        let mut wireproto_logger = self.wireproto_logger(ops::GETTREEPACK, Some(args));

        self.gettreepack_untimed(params)
            .traced(self.trace(), ops::GETTREEPACK, trace_args!())
            .timed(move |stats, _| {
                STATS::gettreepack_ms.add_value(stats.completion_time.as_millis_unchecked() as i64);
                wireproto_logger.finish_stream_wireproto_processing(&stats);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('getfiles', 'files*')
    fn getfiles(&self, params: BoxStream<(HgNodeHash, MPath), Error>) -> BoxStream<Bytes, Error> {
        let logger = self.logger().clone();
        let trace = self.trace().clone();
        info!(logger, "getfiles");

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
                    let args = format!("node: {}, path: {}", node, path);
                    // Logs info about processing of a single file to scuba
                    let mut scuba_logger = this.scuba_logger(ops::GETFILES, Some(args));

                    let repo = this.repo.clone();
                    create_remotefilelog_blob(
                        ctx.clone(),
                        Arc::new(repo.blobrepo().clone()),
                        node,
                        path.clone(),
                        trace.clone(),
                        repo.lfs_params().clone(),
                        scuba_logger.clone(),
                        validate_hash,
                    ).traced(
                        this.trace(),
                        ops::GETFILES,
                        trace_args!("node" => node.to_string(), "path" =>  path.to_string()),
                    )
                        .timed(move |stats, _| {
                            STATS::getfiles_ms
                                .add_value(stats.completion_time.as_millis_unchecked() as i64);
                            scuba_logger
                                .add_future_stats(&stats)
                                .log_with_msg("Command processed", None);
                            Ok(())
                        })
                }
            })
            .buffered(getfiles_buffer_size)
            .timed(move |stats, _| {
                let getfiles_params = getfiles_params.lock().unwrap();
                let mut encoded_params = vec![];
                for (node, path) in getfiles_params.iter() {
                    encoded_params.push(vec![
                        format!("{}", node),
                        String::from_utf8_lossy(&path.to_vec()).to_string(),
                    ]);
                }
                wireproto_logger.set_args(Some(json!{encoded_params}));
                wireproto_logger.finish_stream_wireproto_processing(&stats);
                Ok(())
            })
            .boxify()
    }

    // @wireprotocommand('stream_out_shallow')
    fn stream_out_shallow(&self) -> BoxStream<Bytes, Error> {
        info!(self.logger(), "stream_out_shallow");
        let changelog = match self.repo.streaming_clone() {
            None => Ok(RevlogStreamingChunks::new()).into_future().left_future(),
            Some(SqlStreamingCloneConfig {
                blobstore,
                fetcher,
                repoid,
            }) => fetcher
                .fetch_changelog(*repoid, blobstore.clone())
                .right_future(),
        };

        changelog
            .map({
                let logger = self.logger().clone();
                move |changelog_chunks| {
                    debug!(
                        logger,
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
            .boxify()
    }
}

fn get_changed_manifests_stream(
    repo: &BlobRepo,
    mfid: &HgNodeHash,
    basemfid: &HgNodeHash,
    rootpath: Option<MPath>,
    pruner: impl Pruner + Send + Clone + 'static,
    max_depth: usize,
    trace: TraceContext,
) -> BoxStream<(Box<Entry + Sync>, Option<MPath>), Error> {
    let mfid = HgManifestId::new(*mfid);
    let manifest = repo.get_manifest_by_nodeid(&mfid)
        .traced(&trace, "fetch rootmf", trace_args!());
    let basemfid = HgManifestId::new(*basemfid);
    let basemanifest =
        repo.get_manifest_by_nodeid(&basemfid)
            .traced(&trace, "fetch baserootmf", trace_args!());

    let root_entry_stream = stream::once(Ok((repo.get_root_entry(&mfid), rootpath.clone())));

    if max_depth == 1 {
        return root_entry_stream.boxify();
    }

    let changed_entries = manifest
        .join(basemanifest)
        .map({
            let rootpath = rootpath.clone();
            move |(mf, basemf)| {
                changed_entry_stream_with_pruner(&mf, &basemf, rootpath, pruner, Some(max_depth))
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
    trace: TraceContext,
    validate_content: bool,
    mut scuba_logger: ScubaSampleBuilder,
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

    let parents = entry.get_parents().traced(
        &trace,
        "fetching parents",
        trace_args!(
            "node" => node.to_string(),
            "path" => path.to_string()
        ),
    );

    let linknode_fut = repo.get_linknode(ctx, &repo_path, &entry.get_hash().into_nodehash())
        .traced(
            &trace,
            "fetching linknode",
            trace_args!(
                "node" => node.to_string(),
                "path" => path.to_string()
            ),
        );

    let content_fut = entry
        .get_raw_content()
        .map(|blob| blob.into_inner())
        .traced(
            &trace,
            "fetching raw content",
            trace_args!(
                "node" => node.to_string(),
                "path" => path.to_string()
            ),
        );

    let validate_content = if validate_content {
        entry
            .get_raw_content()
            .join(entry.get_parents())
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
                    scuba_logger.log_with_msg("Data corruption", Some(error_msg));
                    Err(ErrorKind::DataCorruption {
                        path,
                        expected,
                        actual,
                    }.into())
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
        .map(move |((parents, linknode), content)| {
            let (p1, p2) = parents.get_nodes();
            parts::TreepackPartInput {
                node: node.into_nodehash(),
                p1: p1.cloned(),
                p2: p2.cloned(),
                content,
                name: entry.get_name().cloned(),
                linknode: linknode.into_nodehash(),
                basepath,
            }
        })
        .boxify()
}
