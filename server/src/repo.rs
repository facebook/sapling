// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! State for a single source control Repo

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::{self, Debug};
use std::io::{Cursor, Write};
use std::iter::FromIterator;
use std::mem;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use bytes::{BufMut, Bytes, BytesMut};
use failure::err_msg;
use fbwhoami::FbWhoAmI;
use futures::{future, stream, Async, Future, IntoFuture, Poll, Stream, future::Either};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use futures_stats::{Stats, Timed};
use futures_trace::{self, Traced};
use itertools::Itertools;
use pylz4;
use rand::Isaac64Rng;
use rand::distributions::{Distribution, LogNormal};
use scuba::{ScubaClient, ScubaSample};
use time_ext::DurationExt;
use tokio_core::reactor::Remote;

use slog::{self, Drain, Logger};
use slog_scuba::ScubaDrain;

use blobrepo::BlobChangeset;
use bundle2_resolver;
use filenodes::FilenodeInfo;
use mercurial::{self, RevlogChangeset};
use mercurial_bundles::{parts, Bundle2EncodeBuilder, Bundle2Item};
use mercurial_types::{percent_encode, Changeset, Entry, HgBlobNode, HgChangesetId, HgManifestId,
                      HgNodeHash, HgParents, MPath, RepoPath, RepositoryId, Type, NULL_HASH};
use mercurial_types::manifest_utils::{changed_entry_stream, changed_entry_stream_with_pruner,
                                      visited_pruner, ChangedEntry, EntryStatus};
use metaconfig::repoconfig::RepoType;

use hgproto::{self, GetbundleArgs, GettreepackArgs, HgCommandRes, HgCommands};

use blobrepo::BlobRepo;

use errors::*;

use repoinfo::RepoGenCache;
use revset::DifferenceOfUnionsOfAncestorsNodeStream;

const METAKEYFLAG: &str = "f";
const METAKEYSIZE: &str = "s";
const MAX_NODES_TO_LOG: usize = 5;

lazy_static! {
    static ref WHOAMI: Option<FbWhoAmI> = FbWhoAmI::new().ok();
}

mod ops {
    pub const HELLO: &str = "hello";
    pub const UNBUNDLE: &str = "unbundle";
    pub const HEADS: &str = "heads";
    pub const LOOKUP: &str = "lookup";
    pub const KNOWN: &str = "known";
    pub const BETWEEN: &str = "between";
    pub const GETBUNDLE: &str = "getbundle";
    pub const GETTREEPACK: &str = "gettreepack";
    pub const GETFILES: &str = "getfiles";
}

pub fn init_repo(
    parent_logger: &Logger,
    repotype: &RepoType,
    cache_size: usize,
    remote: &Remote,
    repoid: RepositoryId,
    scuba_table: Option<String>,
) -> Result<(PathBuf, MononokeRepo)> {
    let repopath = repotype.path();

    let mut sock = repopath.join(".hg");

    let repo = MononokeRepo::new(
        parent_logger,
        repotype,
        cache_size,
        remote,
        repoid,
        scuba_table,
    ).with_context(|_| format!("Failed to initialize repo {:?}", repopath))?;

    sock.push("mononoke.sock");

    Ok((sock, repo))
}

struct LogNormalGenerator {
    rng: Isaac64Rng,
    distribution: LogNormal,
}

pub trait OpenableRepoType {
    fn open(&self, logger: Logger, remote: &Remote, repoid: RepositoryId) -> Result<BlobRepo>;
    fn path(&self) -> &Path;
}

impl OpenableRepoType for RepoType {
    fn open(&self, logger: Logger, _remote: &Remote, repoid: RepositoryId) -> Result<BlobRepo> {
        use hgproto::ErrorKind;
        use metaconfig::repoconfig::RepoType::*;

        let ret = match *self {
            Revlog(_) => Err(ErrorKind::CantServeRevlogRepo)?,
            BlobRocks(ref path) => BlobRepo::new_rocksdb(logger, &path, repoid)?,
            TestBlobManifold {
                ref manifold_bucket,
                ref prefix,
                ref db_address,
                ref blobstore_cache_size,
                ref changesets_cache_size,
                ref filenodes_cache_size,
                ref io_thread_num,
                ref max_concurrent_requests_per_io_thread,
                ..
            } => BlobRepo::new_test_manifold(
                logger,
                manifold_bucket,
                &prefix,
                repoid,
                &db_address,
                *blobstore_cache_size,
                *changesets_cache_size,
                *filenodes_cache_size,
                *io_thread_num,
                *max_concurrent_requests_per_io_thread,
            )?,
            TestBlobDelayRocks(ref path, mean, stddev) => {
                // We take in an arithmetic mean and stddev, and deduce a log normal
                let mean = mean as f64 / 1_000_000.0;
                let stddev = stddev as f64 / 1_000_000.0;
                let variance = stddev * stddev;
                let mean_squared = mean * mean;

                let mu = (mean_squared / (variance + mean_squared).sqrt()).ln();
                let sigma = (1.0 + variance / mean_squared).ln();

                let max_delay = 16.0;

                let mut delay_gen = LogNormalGenerator {
                    // This is a deterministic RNG if not seeded
                    rng: Isaac64Rng::new_from_u64(0),
                    distribution: LogNormal::new(mu, sigma),
                };
                let delay_gen = move |()| {
                    let delay = delay_gen.distribution.sample(&mut delay_gen.rng);
                    let delay = if delay < 0.0 || delay > max_delay {
                        max_delay
                    } else {
                        delay
                    };
                    let seconds = delay as u64;
                    let nanos = ((delay - seconds as f64) * 1_000_000_000.0) as u32;
                    Duration::new(seconds, nanos)
                };
                BlobRepo::new_rocksdb_delayed(
                    logger,
                    &path,
                    repoid,
                    delay_gen,
                    // Roundtrips to the server - i.e. how many delays to apply
                    2, // get
                    3, // put
                    2, // is_present
                    2, // assert_present
                )?
            }
        };

        Ok(ret)
    }

    fn path(&self) -> &Path {
        use metaconfig::repoconfig::RepoType::*;

        match *self {
            Revlog(ref path) | BlobRocks(ref path) => path.as_ref(),
            TestBlobManifold { ref path, .. } => path.as_ref(),
            TestBlobDelayRocks(ref path, ..) => path.as_ref(),
        }
    }
}

fn format_nodes_list(mut nodes: Vec<HgNodeHash>) -> String {
    nodes.sort();
    nodes.into_iter().map(|node| format!("{}", node)).join(" ")
}

fn format_utf8_bytes_list(mut entries: Vec<Bytes>) -> String {
    entries.sort();
    entries
        .into_iter()
        .map(|entry| String::from_utf8_lossy(&entry).into_owned())
        .join(" ")
}

fn add_common_stats_and_send_to_scuba(
    scuba: Option<Arc<ScubaClient>>,
    mut sample: ScubaSample,
    stats: Stats,
    remote: Remote,
    args: Option<String>,
) -> BoxFuture<(), ()> {
    let trace_upload = if futures_trace::global::is_enabled() {
        futures_trace::global::context()
            .upload_trace_async(remote)
            .map(|id| Some(id))
            .boxify()
    } else {
        Ok(None).into_future().boxify()
    };

    trace_upload
        .then(move |res| {
            let trace_id = res.unwrap_or_else(|_| Some("upload failed".into()));
            if let Some(ref scuba) = scuba {
                sample.add(
                    "time_elapsed_ms",
                    stats.completion_time.as_millis_unchecked(),
                );
                sample.add("poll_time_us", stats.poll_time.as_micros_unchecked());
                sample.add("poll_count", stats.poll_count);
                if let Some(trace_id) = trace_id {
                    sample.add("trace", trace_id);
                }
                if let Some(args) = args {
                    sample.add("args", args);
                }

                if let &Some(ref whoami) = &*WHOAMI {
                    if let Some(hostname) = whoami.get_name() {
                        sample.add("server_hostname", hostname);
                    }
                }

                scuba.log(&sample);
            }
            Ok(())
        })
        .boxify()
}

pub struct MononokeRepo {
    path: String,
    blobrepo: Arc<BlobRepo>,
    repo_generation: RepoGenCache,
    scuba: Option<Arc<ScubaClient>>,
    remote: Remote,
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

impl MononokeRepo {
    pub fn new(
        parent_logger: &Logger,
        repo: &RepoType,
        cache_size: usize,
        remote: &Remote,
        repoid: RepositoryId,
        scuba_table: Option<String>,
    ) -> Result<Self> {
        let path = repo.path().to_owned();
        let logger = {
            let kv = o!("repo" => format!("{}", path.display()));
            match scuba_table {
                Some(ref table) => {
                    let scuba_drain = ScubaDrain::new(table.clone());
                    let duplicate_drain = slog::Duplicate::new(scuba_drain, parent_logger.clone());
                    Logger::root(duplicate_drain.fuse(), kv)
                }
                None => parent_logger.new(kv),
            }
        };

        Ok(MononokeRepo {
            path: format!("{}", path.display()),
            blobrepo: Arc::new(repo.open(logger, remote, repoid)?),
            repo_generation: RepoGenCache::new(cache_size),
            scuba: match scuba_table {
                Some(name) => Some(Arc::new(ScubaClient::new(name))),
                None => None,
            },
            remote: remote.clone(),
        })
    }

    pub fn path(&self) -> &String {
        &self.path
    }

    pub fn blobrepo(&self) -> Arc<BlobRepo> {
        self.blobrepo.clone()
    }

    fn scuba_sample(&self, op: &str) -> ScubaSample {
        let mut sample = ScubaSample::new();
        sample.add("operation", op);
        sample
    }
}

impl Debug for MononokeRepo {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Repo({})", self.path)
    }
}

pub struct RepoClient {
    repo: Arc<MononokeRepo>,
    logger: Logger,
}

impl RepoClient {
    pub fn new(repo: Arc<MononokeRepo>, parent_logger: Logger) -> Self {
        RepoClient {
            repo: repo,
            logger: parent_logger,
        }
    }

    #[allow(dead_code)]
    pub fn get_logger(&self) -> &Logger {
        &self.logger
    }

    fn create_bundle(&self, args: GetbundleArgs) -> hgproto::Result<HgCommandRes<Bytes>> {
        let writer = Cursor::new(Vec::new());
        let mut bundle = Bundle2EncodeBuilder::new(writer);
        // Mercurial currently hangs while trying to read compressed bundles over the wire:
        // https://bz.mercurial-scm.org/show_bug.cgi?id=5646
        // TODO: possibly enable compression support once this is fixed.
        bundle.set_compressor_type(None);

        let repo_generation = &self.repo.repo_generation;
        let blobrepo = &self.repo.blobrepo;

        let common_heads: HashSet<_> = HashSet::from_iter(args.common.iter());

        let heads: Vec<_> = args.heads
            .iter()
            .filter(|head| !common_heads.contains(head))
            .cloned()
            .collect();

        info!(self.logger, "{} heads requested", heads.len());
        for head in heads.iter() {
            debug!(self.logger, "{}", head);
        }

        let excludes: Vec<_> = args.common
            .iter()
            .map(|node| node.clone().into_option())
            .filter_map(|maybe_node| maybe_node)
            .collect();
        let nodestosend = DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
            &blobrepo,
            repo_generation.clone(),
            heads,
            excludes,
        ).boxify();

        // TODO(stash): avoid collecting all the changelogs in the vector - T25767311
        let nodestosend = nodestosend
            .collect()
            .map(|nodes| stream::iter_ok(nodes.into_iter().rev()))
            .flatten_stream();

        let buffer_size = 100; // TODO(stash): make it configurable
        let changelogentries = nodestosend
            .map({
                let blobrepo = blobrepo.clone();
                move |node| {
                    blobrepo
                        .get_changeset_by_changesetid(&HgChangesetId::new(node))
                        .map(move |cs| (node, cs))
                }
            })
            .buffered(buffer_size)
            .and_then(|(node, cs)| {
                let revlogcs = RevlogChangeset::new_from_parts(
                    cs.parents().clone(),
                    cs.manifestid().clone(),
                    cs.user().into(),
                    cs.time().clone(),
                    cs.extra().clone(),
                    cs.files().into(),
                    cs.comments().into(),
                );

                let mut v = Vec::new();
                mercurial::changeset::serialize_cs(&revlogcs, &mut v)?;
                Ok((
                    node,
                    HgBlobNode::new(Bytes::from(v), revlogcs.p1(), revlogcs.p2()),
                ))
            });

        bundle.add_part(parts::changegroup_part(changelogentries)?);

        // XXX Note that listkeys is NOT returned as a bundle2 capability -- see comment in
        // bundle2caps() for why.

        // TODO: generalize this to other listkey types
        // (note: just calling &b"bookmarks"[..] doesn't work because https://fburl.com/0p0sq6kp)
        if args.listkeys.contains(&b"bookmarks".to_vec()) {
            let blobrepo = self.repo.blobrepo.clone();
            let items = blobrepo.get_bookmarks().map(|(name, cs)| {
                let hash: Vec<u8> = cs.into_nodehash().to_hex().into();
                (name.to_string(), hash)
            });
            bundle.add_part(parts::listkey_part("bookmarks", items)?);
        }
        // TODO(stash): handle includepattern= and excludepattern=

        let encode_fut = bundle.build();

        Ok(encode_fut
            .map(|cursor| Bytes::from(cursor.into_inner()))
            .from_err()
            .boxify())
    }

    fn gettreepack_untimed(&self, params: GettreepackArgs) -> HgCommandRes<Bytes> {
        info!(self.logger, "gettreepack {:?}", params);

        if !params.directories.is_empty() {
            // This param is not used by core hg, don't worry about implementing it now
            return Err(err_msg("directories param is not supported"))
                .into_future()
                .boxify();
        }

        // TODO(stash): T25850889 only one basemfnodes is used. That means that trees that client
        // already has can be sent to the client.
        let basemfnode = params.basemfnodes.get(0).cloned().unwrap_or(NULL_HASH);

        let rootpath = if params.rootdir.is_empty() {
            None
        } else {
            Some(try_boxfuture!(MPath::new(params.rootdir)))
        };

        let writer = Cursor::new(Vec::new());
        let mut bundle = Bundle2EncodeBuilder::new(writer);
        // Mercurial currently hangs while trying to read compressed bundles over the wire:
        // https://bz.mercurial-scm.org/show_bug.cgi?id=5646
        // TODO: possibly enable compression support once this is fixed.
        bundle.set_compressor_type(None);

        let pruner = if params.mfnodes.len() > 1 {
            Some(visited_pruner())
        } else {
            None
        };

        let changed_entries = params.mfnodes.iter().fold(
            stream::empty().boxify(),
            move |cur_stream, manifest_id| {
                let new_stream = get_changed_entry_stream(
                    self.repo.blobrepo.clone(),
                    &manifest_id,
                    &basemfnode,
                    rootpath.clone(),
                    pruner.clone(),
                );
                cur_stream.select(new_stream).boxify()
            },
        );

        let changed_entries = changed_entries
            .filter({
                let mut used_hashes = HashSet::new();
                move |entry| used_hashes.insert(*entry.0.get_hash())
            })
            .map({
                let blobrepo = self.repo.blobrepo.clone();
                move |(entry, basepath)| {
                    fetch_treepack_part_input(blobrepo.clone(), entry, basepath)
                }
            });

        parts::treepack_part(changed_entries)
            .into_future()
            .and_then(|part| {
                bundle.add_part(part);
                bundle.build()
            })
            .map(|cursor| Bytes::from(cursor.into_inner()))
            .from_err()
            .boxify()
    }
}

impl HgCommands for RepoClient {
    // @wireprotocommand('between', 'pairs')
    fn between(&self, pairs: Vec<(HgNodeHash, HgNodeHash)>) -> HgCommandRes<Vec<Vec<HgNodeHash>>> {
        info!(self.logger, "between pairs {:?}", pairs);

        struct ParentStream<CS> {
            repo: Arc<MononokeRepo>,
            n: HgNodeHash,
            bottom: HgNodeHash,
            wait_cs: Option<CS>,
        };

        impl<CS> ParentStream<CS> {
            fn new(repo: &Arc<MononokeRepo>, top: HgNodeHash, bottom: HgNodeHash) -> Self {
                ParentStream {
                    repo: repo.clone(),
                    n: top,
                    bottom: bottom,
                    wait_cs: None,
                }
            }
        }

        impl Stream for ParentStream<BoxFuture<BlobChangeset, hgproto::Error>> {
            type Item = HgNodeHash;
            type Error = hgproto::Error;

            fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
                if self.n == self.bottom || self.n == NULL_HASH {
                    return Ok(Async::Ready(None));
                }

                self.wait_cs = self.wait_cs.take().or_else(|| {
                    Some(
                        self.repo
                            .blobrepo
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

        let scuba = self.repo.scuba.clone();
        let sample = self.repo.scuba_sample(ops::BETWEEN);
        let remote = self.repo.remote.clone();

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
            .timed(move |stats, _| {
                add_common_stats_and_send_to_scuba(scuba, sample, stats, remote, None)
            })
            .boxify()
    }

    // @wireprotocommand('heads')
    fn heads(&self) -> HgCommandRes<HashSet<HgNodeHash>> {
        // Get a stream of heads and collect them into a HashSet
        // TODO: directly return stream of heads
        let logger = self.logger.clone();
        let scuba = self.repo.scuba.clone();
        let sample = self.repo.scuba_sample(ops::HEADS);
        let remote = self.repo.remote.clone();
        self.repo
            .blobrepo
            .get_heads()
            .collect()
            .map(|v| v.into_iter().collect())
            .from_err()
            .inspect(move |resp| debug!(logger, "heads response: {:?}", resp))
            .timed(move |stats, _| {
                add_common_stats_and_send_to_scuba(scuba, sample, stats, remote, None)
            })
            .boxify()
    }

    // @wireprotocommand('lookup', 'key')
    fn lookup(&self, key: String) -> HgCommandRes<Bytes> {
        info!(self.logger, "lookup: {:?}", key);
        // TODO(stash): T25928839 lookup should support bookmarks and prefixes too
        let repo = self.repo.blobrepo.clone();
        let scuba = self.repo.scuba.clone();
        let sample = self.repo.scuba_sample(ops::LOOKUP);
        let remote = self.repo.remote.clone();
        HgNodeHash::from_str(&key)
            .into_future()
            .and_then(move |node| {
                let csid = HgChangesetId::new(node);
                repo.changeset_exists(&csid)
                    .map(move |exists| (node, exists))
            })
            .and_then(|(node, exists)| {
                if exists {
                    let mut buf = BytesMut::with_capacity(node.to_hex().len() + 3);
                    buf.put(b'1');
                    buf.put(b' ');
                    buf.extend_from_slice(node.to_hex().as_bytes());
                    buf.put(b'\n');
                    Ok(buf.freeze())
                } else {
                    let err_msg = format!("{} not found", node);
                    let mut buf = BytesMut::with_capacity(err_msg.len() + 3);
                    buf.put(b'0');
                    buf.put(b' ');
                    buf.extend_from_slice(err_msg.as_bytes());
                    buf.put(b'\n');
                    Ok(buf.freeze())
                }
            })
            .timed(move |stats, _| {
                add_common_stats_and_send_to_scuba(scuba, sample, stats, remote, None)
            })
            .boxify()
    }

    // @wireprotocommand('known', 'nodes *'), but the '*' is ignored
    fn known(&self, nodes: Vec<HgNodeHash>) -> HgCommandRes<Vec<bool>> {
        if nodes.len() > MAX_NODES_TO_LOG {
            info!(self.logger, "known: {:?}...", &nodes[..MAX_NODES_TO_LOG]);
        } else {
            info!(self.logger, "known: {:?}", nodes);
        }
        let blobrepo = self.repo.blobrepo.clone();
        let scuba = self.repo.scuba.clone();
        let sample = self.repo.scuba_sample(ops::KNOWN);
        let remote = self.repo.remote.clone();

        future::join_all(
            nodes
                .into_iter()
                .map(move |node| blobrepo.changeset_exists(&HgChangesetId::new(node))),
        ).timed(move |stats, _| {
            add_common_stats_and_send_to_scuba(scuba, sample, stats, remote, None)
        })
            .boxify()
    }

    // @wireprotocommand('getbundle', '*')
    fn getbundle(&self, args: GetbundleArgs) -> HgCommandRes<Bytes> {
        info!(self.logger, "Getbundle: {:?}", args);

        let scuba = self.repo.scuba.clone();
        let sample = self.repo.scuba_sample(ops::GETBUNDLE);
        let remote = self.repo.remote.clone();

        match self.create_bundle(args) {
            Ok(res) => res,
            Err(err) => Err(err).into_future().boxify(),
        }.timed(move |stats, _| {
            add_common_stats_and_send_to_scuba(scuba, sample, stats, remote, None)
        })
            .boxify()
    }

    // @wireprotocommand('hello')
    fn hello(&self) -> HgCommandRes<HashMap<String, Vec<String>>> {
        info!(self.logger, "Hello -> capabilities");

        let mut res = HashMap::new();
        let mut caps = wireprotocaps();
        caps.push(format!("bundle2={}", bundle2caps()));
        res.insert("capabilities".to_string(), caps);

        let scuba = self.repo.scuba.clone();
        let sample = self.repo.scuba_sample(ops::HELLO);
        let remote = self.repo.remote.clone();
        future::ok(res)
            .timed(move |stats, _| {
                add_common_stats_and_send_to_scuba(scuba, sample, stats, remote, None)
            })
            .boxify()
    }

    // @wireprotocommand('listkeys', 'namespace')
    fn listkeys(&self, namespace: String) -> HgCommandRes<HashMap<Vec<u8>, Vec<u8>>> {
        if namespace == "bookmarks" {
            self.repo
                .blobrepo
                .get_bookmarks()
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
                .boxify()
        } else {
            info!(
                self.get_logger(),
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
    ) -> HgCommandRes<Bytes> {
        let res = bundle2_resolver::resolve(
            self.repo.blobrepo.clone(),
            self.logger.new(o!("command" => "unbundle")),
            heads,
            stream,
        );

        let scuba = self.repo.scuba.clone();
        let sample = self.repo.scuba_sample(ops::UNBUNDLE);
        let remote = self.repo.remote.clone();

        res.traced_global("unbundle", trace_args!())
            .timed(move |stats, _| {
                add_common_stats_and_send_to_scuba(scuba, sample, stats, remote, None)
            })
            .boxify()
    }

    // @wireprotocommand('gettreepack', 'rootdir mfnodes basemfnodes directories')
    fn gettreepack(&self, params: GettreepackArgs) -> HgCommandRes<Bytes> {
        let scuba = self.repo.scuba.clone();
        let sample = self.repo.scuba_sample(ops::GETTREEPACK);
        let remote = self.repo.remote.clone();

        return self.gettreepack_untimed(params.clone())
            .traced_global(
                "gettreepack",
                trace_args!(
                    "rootdir" => String::from_utf8_lossy(&params.rootdir),
                    "mfnodes" => format_nodes_list(params.mfnodes.clone()),
                    "basemfnodes" => format_nodes_list(params.basemfnodes.clone()),
                    "directories" => format_utf8_bytes_list(params.directories.clone()),
                ),
            )
            .timed(move |stats, _| {
                let args = format!(
                    "rootdir: {}, mfnodes: {}, basemfnodes: {}, directories: {}",
                    String::from_utf8_lossy(&params.rootdir),
                    format_nodes_list(params.mfnodes),
                    format_nodes_list(params.basemfnodes),
                    format_utf8_bytes_list(params.directories),
                );
                add_common_stats_and_send_to_scuba(scuba, sample, stats, remote, Some(args))
            })
            .boxify();
    }

    // @wireprotocommand('getfiles', 'files*')
    fn getfiles(&self, params: BoxStream<(HgNodeHash, MPath), Error>) -> BoxStream<Bytes, Error> {
        let logger = self.logger.clone();
        info!(logger, "getfiles");
        let repo = self.repo.clone();
        let getfiles_buffer_size = 100; // TODO(stash): make it configurable
        params
            .map(move |(node, path)| {
                trace!(logger, "get file request: {:?} {}", path, node);
                let repo = repo.clone();
                create_remotefilelog_blob(repo.blobrepo.clone(), node, path.clone())
                    .traced_global(
                        "getfile",
                        trace_args!("node" => format!("{}", node), "path" => format!("{}", path)),
                    )
                    .timed(move |stats, _| {
                        let sample = repo.scuba_sample(ops::GETFILES);
                        let args = format!("node: {}, path: {}", node, path);
                        add_common_stats_and_send_to_scuba(
                            repo.scuba.clone(),
                            sample,
                            stats,
                            repo.remote.clone(),
                            Some(args),
                        )
                    })
            })
            .buffered(getfiles_buffer_size)
            .boxify()
    }
}

fn get_changed_entry_stream(
    repo: Arc<BlobRepo>,
    mfid: &HgNodeHash,
    basemfid: &HgNodeHash,
    rootpath: Option<MPath>,
    pruner: Option<impl FnMut(&ChangedEntry) -> bool + Send + Clone + 'static>,
) -> BoxStream<(Box<Entry + Sync>, Option<MPath>), Error> {
    let manifest = repo.get_manifest_by_nodeid(mfid)
        .traced_global("fetch rootmf", trace_args!());
    let basemanifest = repo.get_manifest_by_nodeid(basemfid)
        .traced_global("fetch baserootmf", trace_args!());

    let changed_entries = manifest
        .join(basemanifest)
        .map({
            let rootpath = rootpath.clone();
            move |(mf, basemf)| match pruner {
                Some(pruner) => {
                    changed_entry_stream_with_pruner(&mf, &basemf, rootpath, pruner).boxify()
                }
                None => changed_entry_stream(&mf, &basemf, rootpath).boxify(),
            }
        })
        .flatten_stream();

    let changed_entries =
        changed_entries.filter_map(move |entry_status| match entry_status.status {
            EntryStatus::Added(entry) => {
                if entry.get_type() == Type::Tree {
                    Some((entry, entry_status.dirname))
                } else {
                    None
                }
            }
            EntryStatus::Modified { to_entry, .. } => {
                if to_entry.get_type() == Type::Tree {
                    Some((to_entry, entry_status.dirname))
                } else {
                    None
                }
            }
            EntryStatus::Deleted(..) => None,
        });

    // Append root manifest
    let root_entry_stream = stream::once(Ok((
        repo.get_root_entry(&HgManifestId::new(*mfid)),
        rootpath,
    )));

    changed_entries.chain(root_entry_stream).boxify()
}

fn fetch_treepack_part_input(
    repo: Arc<BlobRepo>,
    entry: Box<Entry + Sync>,
    basepath: Option<MPath>,
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

    let parents = entry.get_parents().traced_global(
        "fetching parents",
        trace_args!(
                "node" => format!("{}", node),
                "path" => format!("{}", path)
            ),
    );

    let linknode_fut = repo.get_linknode(repo_path, &entry.get_hash().into_nodehash())
        .traced_global(
            "fetching linknode",
            trace_args!(
                "node" => format!("{}", node),
                "path" => format!("{}", path)
            ),
        );

    let content_fut = entry
        .get_raw_content()
        .and_then(|blob| blob.into_inner().ok_or(err_msg("bad blob content")))
        .traced_global(
            "fetching raw content",
            trace_args!(
                "node" => format!("{}", node),
                "path" => format!("{}", path)
            ),
        );

    parents
        .join(linknode_fut)
        .join(content_fut)
        .map(move |((parents, linknode), content)| {
            let (p1, p2) = parents.get_nodes();
            parts::TreepackPartInput {
                node: node.into_nodehash(),
                p1: p1.cloned(),
                p2: p2.cloned(),
                content,
                name: entry.get_name().cloned(),
                linknode,
                basepath,
            }
        })
        .boxify()
}

fn get_file_history(
    repo: Arc<BlobRepo>,
    startnode: HgNodeHash,
    path: MPath,
    prefetched_history: HashMap<HgNodeHash, FilenodeInfo>,
) -> BoxStream<
    (
        HgNodeHash,
        HgParents,
        HgNodeHash,
        Option<(MPath, HgNodeHash)>,
    ),
    Error,
> {
    if startnode == NULL_HASH {
        return stream::empty().boxify();
    }
    let mut startstate = VecDeque::new();
    startstate.push_back(startnode);
    let seen_nodes: HashSet<_> = [startnode].iter().cloned().collect();
    let path = RepoPath::FilePath(path);

    stream::unfold(
        (startstate, seen_nodes),
        move |cur_data: (VecDeque<HgNodeHash>, HashSet<HgNodeHash>)| {
            let (mut nodes, mut seen_nodes) = cur_data;
            let node = nodes.pop_front()?;

            let futs = if prefetched_history.contains_key(&node) {
                let filenode = prefetched_history.get(&node).unwrap();

                let p1 = filenode.p1.map(|p| p.into_nodehash());
                let p2 = filenode.p2.map(|p| p.into_nodehash());
                let parents = Ok(HgParents::new(p1.as_ref(), p2.as_ref())).into_future();

                let linknode = Ok(filenode.linknode.into_nodehash()).into_future();

                let copy =
                    Ok(filenode
                        .copyfrom
                        .clone()
                        .map(|(frompath, node)| (frompath, node.into_nodehash())))
                        .into_future();

                (Either::A(parents), Either::A(linknode), Either::A(copy))
            } else {
                let parents = repo.get_parents(&path, &node);
                let parents = parents.traced_global("fetching parents", trace_args!());

                let copy = repo.get_file_copy(&path, &node);
                let copy = copy.traced_global("fetching copy info", trace_args!());

                let linknode = Ok(path.clone()).into_future().and_then({
                    let repo = repo.clone();
                    move |path| repo.get_linknode(path, &node)
                });
                let linknode = linknode.traced_global("fetching linknode info", trace_args!());

                (Either::B(parents), Either::B(linknode), Either::B(copy))
            };

            let (parents, linknode, copy) = futs;

            let copy = copy.and_then({
                let path = path.clone();
                move |filecopy| match filecopy {
                    Some((RepoPath::FilePath(copyto), rev)) => Ok(Some((copyto, rev))),
                    Some((copyto, _)) => Err(ErrorKind::InconsistenCopyInfo(path, copyto).into()),
                    None => Ok(None),
                }
            });

            let joined = parents
                .join(linknode)
                .join(copy)
                .map(|(pl, c)| (pl.0, pl.1, c));

            Some(joined.map(move |(parents, linknode, copy)| {
                nodes.extend(parents.into_iter().filter(|p| seen_nodes.insert(*p)));
                ((node, parents, linknode, copy), (nodes, seen_nodes))
            }))
        },
    ).boxify()
}

/// Remotefilelog blob consists of file content in `node` revision and all the history
/// of the file up to `node`
fn create_remotefilelog_blob(
    repo: Arc<BlobRepo>,
    node: HgNodeHash,
    path: MPath,
) -> BoxFuture<Bytes, Error> {
    // raw_content includes copy information
    let raw_content_bytes = repo.get_file_content(&node)
        .and_then(move |raw_content| {
            let raw_content = raw_content.into_bytes();
            // requires digit counting to know for sure, use reasonable approximation
            let approximate_header_size = 12;
            let mut writer = Cursor::new(Vec::with_capacity(
                approximate_header_size + raw_content.len(),
            ));

            // Write header
            // TODO(stash): support LFS files using METAKEYFLAG
            let res = write!(
                writer,
                "v1\n{}{}\n{}{}\0",
                METAKEYSIZE,
                raw_content.len(),
                METAKEYFLAG,
                0,
            );

            res.and_then(|_| writer.write_all(&raw_content))
                .map_err(Error::from)
                .map(|_| writer.into_inner())
        })
        .traced_global("fetching remotefilelog content", trace_args!());

    // Do bulk prefetch of the filenodes first. That saves lots of db roundtrips.
    // Prefetched filenodes are used as a cache. If filenode is not in the cache, then it will
    // be fetched again.
    let prefetched_filenodes = repo.get_all_filenodes(RepoPath::FilePath(path.clone()))
        .map(|filenodes| {
            filenodes
                .into_iter()
                .map(|filenode| (filenode.filenode.into_nodehash(), filenode))
                .collect()
        });

    let file_history_bytes = prefetched_filenodes
        .and_then({
            let node = node.clone();
            move |prefetched_filenodes| {
                get_file_history(repo, node, path, prefetched_filenodes).collect()
            }
        })
        .and_then(|history| {
            let approximate_history_entry_size = 81;
            let mut writer = Cursor::new(Vec::with_capacity(
                history.len() * approximate_history_entry_size,
            ));

            for (node, parents, linknode, copy) in history {
                let (p1, p2) = match parents {
                    HgParents::None => (NULL_HASH, NULL_HASH),
                    HgParents::One(p) => (p, NULL_HASH),
                    HgParents::Two(p1, p2) => (p1, p2),
                };

                let (p1, p2, copied_from) = if let Some((copied_from, copied_rev)) = copy {
                    // Mercurial has a complicated copy/renames logic.
                    // If (path1, filenode1) is copied/renamed from (path2, filenode2),
                    // filenode1's p1 is set to filenode2, and copy_from path is set to path2
                    // filenode1's p2 is null for non-merge commits. It might be non-null for merges.
                    (copied_rev, p1, Some(copied_from))
                } else {
                    (p1, p2, None)
                };

                writer.write_all(node.as_bytes())?;
                writer.write_all(p1.as_bytes())?;
                writer.write_all(p2.as_bytes())?;
                writer.write_all(linknode.as_bytes())?;
                if let Some(copied_from) = copied_from {
                    writer.write_all(&copied_from.to_vec())?;
                }

                write!(writer, "\0")?;
            }
            Ok(writer.into_inner())
        })
        .traced_global("fetching file history", trace_args!());

    raw_content_bytes
        .join(file_history_bytes)
        .map(|(mut raw_content, file_history)| {
            raw_content.extend(file_history);
            raw_content
        })
        .and_then(|content| pylz4::compress(&content))
        .map(|bytes| Bytes::from(bytes))
        .boxify()
}
