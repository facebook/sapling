// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! State for a single source control Repo

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::{self, Debug};
use std::io::{Cursor, Write};
use std::mem;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use bytes::{BufMut, Bytes, BytesMut};
use failure::err_msg;
use futures::{future, stream, Async, Future, IntoFuture, Poll, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use pylz4;
use tokio_core::reactor::Remote;

use slog::Logger;

use blobrepo::BlobChangeset;
use bundle2_resolver;
use mercurial;
use mercurial_bundles::{parts, Bundle2EncodeBuilder, Bundle2Item};
use mercurial_types::{percent_encode, BlobNode, Changeset, ChangesetId, Entry, MPath, ManifestId,
                      NodeHash, Parents, RepoPath, Type, NULL_HASH};
use mercurial_types::manifest_utils::{changed_entry_stream, EntryStatus};
use metaconfig::repoconfig::RepoType;

use hgproto::{self, GetbundleArgs, GettreepackArgs, HgCommandRes, HgCommands};

use blobrepo::BlobRepo;

use errors::*;

use repoinfo::RepoGenCache;
use revset::{AncestorsNodeStream, IntersectNodeStream, NodeStream, SetDifferenceNodeStream,
             SingleNodeHash, UnionNodeStream};

const METAKEYFLAG: &str = "f";
const METAKEYSIZE: &str = "s";

pub fn init_repo(
    parent_logger: &Logger,
    repotype: &RepoType,
    cache_size: usize,
    remote: &Remote,
) -> Result<(PathBuf, HgRepo)> {
    let repopath = repotype.path();

    let mut sock = repopath.join(".hg");

    let repo = HgRepo::new(parent_logger, repotype, cache_size, remote)
        .with_context(|_| format!("Failed to initialize repo {:?}", repopath))?;

    sock.push("mononoke.sock");

    Ok((sock, repo))
}

pub trait OpenableRepoType {
    fn open(&self, remote: &Remote) -> Result<BlobRepo>;
    fn path(&self) -> &Path;
}

impl OpenableRepoType for RepoType {
    fn open(&self, remote: &Remote) -> Result<BlobRepo> {
        use hgproto::ErrorKind;
        use metaconfig::repoconfig::RepoType::*;

        let ret = match *self {
            Revlog(_) => Err(ErrorKind::CantServeRevlogRepo)?,
            BlobFiles(ref path) => BlobRepo::new_files(&path)?,
            BlobRocks(ref path) => BlobRepo::new_rocksdb(&path)?,
            TestBlobManifold(ref bucket, _) => BlobRepo::new_test_manifold(bucket, remote)?,
        };

        Ok(ret)
    }

    fn path(&self) -> &Path {
        use metaconfig::repoconfig::RepoType::*;

        match *self {
            Revlog(ref path) | BlobFiles(ref path) | BlobRocks(ref path) => path.as_ref(),
            TestBlobManifold(_, ref path) => path.as_ref(),
        }
    }
}

pub struct HgRepo {
    path: String,
    hgrepo: Arc<BlobRepo>,
    repo_generation: RepoGenCache,
    _logger: Logger,
}

fn wireprotocaps() -> Vec<String> {
    vec![
        "lookup".to_string(),
        "known".to_string(),
        "getbundle".to_string(),
        "unbundle=HG10GZ,HG10BZ,HG10UN".to_string(),
        "gettreepack".to_string(),
        "remotefilelog".to_string(),
    ]
}

fn bundle2caps() -> String {
    let caps = vec![
        ("HG20", vec![]),
        ("listkeys", vec![]),
        ("changegroup", vec!["02"]),
        ("b2x:infinitepush", vec![]),
        ("b2x:infinitepushscratchbookmarks", vec![]),
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

impl HgRepo {
    pub fn new(
        parent_logger: &Logger,
        repo: &RepoType,
        cache_size: usize,
        remote: &Remote,
    ) -> Result<Self> {
        let path = repo.path().to_owned();

        Ok(HgRepo {
            path: format!("{}", path.display()),
            hgrepo: Arc::new(repo.open(remote)?),
            repo_generation: RepoGenCache::new(cache_size),
            _logger: parent_logger.new(o!("repo" => format!("{}", path.display()))),
        })
    }

    pub fn path(&self) -> &String {
        &self.path
    }
}

impl Debug for HgRepo {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Repo({})", self.path)
    }
}

pub struct RepoClient {
    repo: Arc<HgRepo>,
    logger: Logger,
}

impl RepoClient {
    pub fn new(repo: Arc<HgRepo>, parent_logger: &Logger) -> Self {
        RepoClient {
            repo: repo,
            logger: parent_logger.new(o!()), // connection details?
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
        let hgrepo = &self.repo.hgrepo;

        let ancestors_stream = |nodes: &Vec<NodeHash>| -> Box<NodeStream> {
            let heads_ancestors = nodes.iter().map(|head| {
                AncestorsNodeStream::new(&hgrepo, repo_generation.clone(), *head).boxed()
            });
            Box::new(UnionNodeStream::new(
                &hgrepo,
                repo_generation.clone(),
                heads_ancestors,
            ))
        };

        let heads_ancestors = ancestors_stream(&args.heads);
        let common_ancestors = ancestors_stream(&args.common);

        let nodestosend = Box::new(SetDifferenceNodeStream::new(
            hgrepo,
            repo_generation.clone(),
            heads_ancestors,
            common_ancestors,
        ));

        // TODO(stash): avoid collecting all the changelogs in the vector - T25767311
        let nodestosend = nodestosend
            .collect()
            .map(|nodes| stream::iter_ok(nodes.into_iter().rev()))
            .flatten_stream();

        let changelogentries = nodestosend
            .and_then({
                let hgrepo = hgrepo.clone();
                move |node| hgrepo.get_changeset_by_changesetid(&ChangesetId::new(node))
            })
            .and_then(|cs| {
                let mut v = Vec::new();
                mercurial::changeset::serialize_cs(&cs, &mut v)?;
                let parents = cs.parents().get_nodes();
                Ok(BlobNode::new(v, parents.0, parents.1))
            });

        bundle.add_part(parts::changegroup_part(changelogentries)?);

        // TODO: generalize this to other listkey types
        // (note: just calling &b"bookmarks"[..] doesn't work because https://fburl.com/0p0sq6kp)
        if args.listkeys.contains(&b"bookmarks".to_vec()) {
            let hgrepo = self.repo.hgrepo.clone();
            let bookmark_names = hgrepo.get_bookmark_keys();
            let items = bookmark_names.and_then(move |name| {
                // For each bookmark name, grab the corresponding value.
                hgrepo.get_bookmark_value(&name).and_then(|result| {
                    // If the name somehow wasn't found, it's possible a race happened. where the
                    // bookmark was deleted from underneath. Skip it.
                    // Boxing is necessary here to make the match arms return the same types.
                    match result {
                        Some((hash, _version)) => {
                            // AsciiString doesn't currently implement AsRef<[u8]>, so switch to
                            // Vec which does
                            let hash: Vec<u8> = hash.to_hex().into();
                            Ok((name, hash)).into_future().boxify()
                        }
                        None => future::empty().boxify(),
                    }
                })
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
}

impl HgCommands for RepoClient {
    // @wireprotocommand('between', 'pairs')
    fn between(&self, pairs: Vec<(NodeHash, NodeHash)>) -> HgCommandRes<Vec<Vec<NodeHash>>> {
        info!(self.logger, "between pairs {:?}", pairs);

        struct ParentStream<CS> {
            repo: Arc<HgRepo>,
            n: NodeHash,
            bottom: NodeHash,
            wait_cs: Option<CS>,
        };

        impl<CS> ParentStream<CS> {
            fn new(repo: &Arc<HgRepo>, top: NodeHash, bottom: NodeHash) -> Self {
                ParentStream {
                    repo: repo.clone(),
                    n: top,
                    bottom: bottom,
                    wait_cs: None,
                }
            }
        }

        impl Stream for ParentStream<BoxFuture<BlobChangeset, hgproto::Error>> {
            type Item = NodeHash;
            type Error = hgproto::Error;

            fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
                if self.n == self.bottom || self.n == NULL_HASH {
                    return Ok(Async::Ready(None));
                }

                self.wait_cs = self.wait_cs.take().or_else(|| {
                    Some(
                        self.repo
                            .hgrepo
                            .get_changeset_by_changesetid(&ChangesetId::new(self.n)),
                    )
                });
                let cs = try_ready!(self.wait_cs.as_mut().unwrap().poll());
                self.wait_cs = None; // got it

                let p = match cs.parents() {
                    &Parents::None => NULL_HASH,
                    &Parents::One(ref p) => *p,
                    &Parents::Two(ref p, _) => *p,
                };

                let prev_n = mem::replace(&mut self.n, p);

                Ok(Async::Ready(Some(prev_n)))
            }
        }

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
            .boxify()
    }

    // @wireprotocommand('changegroup', 'roots')
    fn changegroup(&self, roots: Vec<NodeHash>) -> HgCommandRes<()> {
        // TODO: streaming something
        info!(self.logger, "changegroup roots {:?}", roots);

        future::ok(()).boxify()
    }

    // @wireprotocommand('heads')
    fn heads(&self) -> HgCommandRes<HashSet<NodeHash>> {
        // Get a stream of heads and collect them into a HashSet
        // TODO: directly return stream of heads
        self.repo
            .hgrepo
            .get_heads()
            .collect()
            .from_err()
            .and_then(|v| Ok(v.into_iter().collect()))
            .boxify()
    }

    // @wireprotocommand('lookup', 'key')
    fn lookup(&self, key: String) -> HgCommandRes<Bytes> {
        // TODO(stash): T25928839 lookup should support bookmarks and prefixes too
        let repo = self.repo.hgrepo.clone();
        NodeHash::from_str(&key)
            .into_future()
            .and_then(move |node| {
                let csid = ChangesetId::new(node);
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
            .boxify()
    }

    // @wireprotocommand('known', 'nodes *'), but the '*' is ignored
    fn known(&self, nodes: Vec<NodeHash>) -> HgCommandRes<Vec<bool>> {
        info!(self.logger, "known: {:?}", nodes);
        let repo_generation = &self.repo.repo_generation;
        let hgrepo = &self.repo.hgrepo;

        // Ultimately, this block takes all ancestors of heads in this repo intersected with
        // the nodes passed in by the client, and then returns a Vec<bool>, true if the
        // intersection contains the matching node in nodes, false if it does not.
        // Note that revsets are lazy, and will not generate unnecessary nodes.
        hgrepo
            .get_heads()
            // Convert Stream<Heads> into Stream<Ancestors<Heads>>
            .map({
                let repo_generation = repo_generation.clone();
                let hgrepo = hgrepo.clone();
                move |hash| AncestorsNodeStream::new(&hgrepo, repo_generation.clone(), hash).boxed()
            })
            // Convert Stream<Ancestors<Heads>>> into Future<Vec<Ancestors<Heads>>>
            .collect()
            // Do the next few steps inside the Future; the parameter to the closure is
            // Vec<Ancestors<Heads>>
            .map({
                let repo_generation = repo_generation.clone();
                let hgrepo = hgrepo.clone();
                let nodes = nodes.clone();
                move |vec| {
                    // Intersect the union of the Vec<Ancestors<Heads>> that's passed in, with
                    // a union of the known nodes the client asked about.
                    IntersectNodeStream::new(
                        &hgrepo,
                        repo_generation.clone(),
                        vec![
                            // This is the union of all ancestors of heads
                            UnionNodeStream::new(&hgrepo, repo_generation.clone(), vec).boxed(),
                            // This is the union of all passed in nodes.
                            UnionNodeStream::new(
                                &hgrepo,
                                repo_generation,
                                nodes.into_iter().map({
                                    let hgrepo = hgrepo.clone();
                                    move |node| SingleNodeHash::new(node, &hgrepo).boxed()
                                }),
                            ).boxed(),
                        ],
                        // collect() below will result in a Future<Vec<NodeHash>> which is all
                        // nodes that are both an ancestor of a get_heads() head and were
                        // passed in by the client
                    ).collect()
                        .from_err::<hgproto::Error>()
                }
            })
            // We have a Future<Future<Vec<NodeHash>>> - collapse one layer of Future.
            .flatten()
            // Finally, within the Future, use the Vec<NodeHash> that's only nodes that were
            // passed in by the client and that are ancestors of a get_heads() head to convert
            // the Vec of client known nodes to a Vec<bool> telling the client if we also
            // know of the nodes it asked us about.
            .map(move |known| {
                nodes
                    .iter()
                    .map(|node| known.contains(node))
                    .collect::<Vec<bool>>()
            })
            .boxify()
    }

    // @wireprotocommand('getbundle', '*')
    fn getbundle(&self, args: GetbundleArgs) -> HgCommandRes<Bytes> {
        info!(self.logger, "Getbundle: {:?}", args);

        match self.create_bundle(args) {
            Ok(res) => res,
            Err(err) => Err(err).into_future().boxify(),
        }
    }

    // @wireprotocommand('hello')
    fn hello(&self) -> HgCommandRes<HashMap<String, Vec<String>>> {
        info!(self.logger, "Hello -> capabilities");

        let mut res = HashMap::new();
        let mut caps = wireprotocaps();
        caps.push(format!("bundle2={}", bundle2caps()));
        res.insert("capabilities".to_string(), caps);

        future::ok(res).boxify()
    }

    // @wireprotocommand('unbundle', 'heads')
    fn unbundle(
        &self,
        heads: Vec<String>,
        stream: BoxStream<Bundle2Item, Error>,
    ) -> HgCommandRes<Bytes> {
        bundle2_resolver::resolve(
            self.repo.hgrepo.clone(),
            self.logger.new(o!("command" => "unbundle")),
            heads,
            stream,
        )
    }

    // @wireprotocommand('gettreepack', 'rootdir mfnodes basemfnodes directories')
    fn gettreepack(&self, params: GettreepackArgs) -> HgCommandRes<Bytes> {
        info!(self.logger, "gettreepack {:?}", params);

        if !params.directories.is_empty() {
            // This param is not used by core hg, don't worry about implementing it now
            return Err(err_msg("directories param is not supported"))
                .into_future()
                .boxify();
        }

        // TODO(stash): T25850889 only one basemfnodes is used. That means that trees that client
        // already has can be sent to the client.
        let basemfnode = params.basemfnodes.get(0).unwrap_or(&NULL_HASH);

        if params.rootdir.len() != 0 {
            // For now, only root repo
            return Err(err_msg("only empty rootdir is supported"))
                .into_future()
                .boxify();
        }

        let writer = Cursor::new(Vec::new());
        let mut bundle = Bundle2EncodeBuilder::new(writer);
        // Mercurial currently hangs while trying to read compressed bundles over the wire:
        // https://bz.mercurial-scm.org/show_bug.cgi?id=5646
        // TODO: possibly enable compression support once this is fixed.
        bundle.set_compressor_type(None);

        // TODO(stash): T25850889 same entries will be generated over and over again.
        // Potentially it can be very inefficient.
        let changed_entries = params.mfnodes.iter().fold(
            stream::empty().boxify(),
            |cur_stream, manifest_id| {
                let new_stream =
                    get_changed_entry_stream(self.repo.hgrepo.clone(), manifest_id, basemfnode);
                cur_stream.select(new_stream).boxify()
            },
        );

        let changed_entries = changed_entries.filter({
            let mut used_hashes = HashSet::new();
            move |entry| used_hashes.insert(*entry.0.get_hash())
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

    // @wireprotocommand('getfiles', 'files*')
    fn getfiles(&self, params: BoxStream<(NodeHash, MPath), Error>) -> BoxStream<Bytes, Error> {
        info!(self.logger, "getfiles");
        let repo = self.repo.hgrepo.clone();
        params
            .and_then(move |(node, path)| create_remotefilelog_blob(repo.clone(), node, path))
            .boxify()
    }
}

fn get_changed_entry_stream(
    repo: Arc<BlobRepo>,
    mfid: &NodeHash,
    basemfid: &NodeHash,
) -> BoxStream<(Box<Entry + Sync>, NodeHash, MPath), Error> {
    let manifest = repo.get_manifest_by_nodeid(mfid);
    let basemanifest = repo.get_manifest_by_nodeid(basemfid);

    let changed_entries = manifest
        .join(basemanifest)
        .map(|(mf, basemf)| changed_entry_stream(&mf, &basemf, MPath::empty()))
        .flatten_stream();

    let changed_entries = changed_entries
        .filter_map(move |entry_status| match entry_status.status {
            EntryStatus::Added(entry) => {
                if entry.get_type() == Type::Tree {
                    Some((entry, entry_status.path))
                } else {
                    None
                }
            }
            EntryStatus::Modified(entry, _) => {
                if entry.get_type() == Type::Tree {
                    Some((entry, entry_status.path))
                } else {
                    None
                }
            }
            EntryStatus::Deleted(..) => None,
        })
        .and_then({
            let hgrepo = repo.clone();
            move |(entry, path)| fetch_linknode(hgrepo.clone(), entry, path)
        })
        .map(|(entry, linknode, basepath)| (entry, linknode, basepath));

    // Append root manifest
    let root_entry_stream = Ok(repo.get_root_entry(&ManifestId::new(*mfid)))
        .into_future()
        .and_then({
            let hgrepo = repo.clone();
            move |entry| fetch_linknode(hgrepo.clone(), entry, MPath::empty())
        })
        .map(|(entry, linknode, basepath)| stream::once(Ok((entry, linknode, basepath))))
        .flatten_stream();

    changed_entries.chain(root_entry_stream).boxify()
}

fn fetch_linknode(
    repo: Arc<BlobRepo>,
    entry: Box<Entry + Sync>,
    basepath: MPath,
) -> BoxFuture<(Box<Entry + Sync>, NodeHash, MPath), Error> {
    let path = match entry.get_name() {
        &Some(ref name) => {
            let path = basepath.clone().join(name.clone().into_iter());
            if entry.get_type() == Type::Tree {
                RepoPath::DirectoryPath(path)
            } else {
                RepoPath::FilePath(path)
            }
        }
        &None => RepoPath::RootPath,
    };

    let linknode_fut = repo.get_linknode(path, &entry.get_hash().into_nodehash());
    linknode_fut
        .map(|linknode| (entry, linknode, basepath))
        .boxify()
}

fn get_file_history(
    repo: Arc<BlobRepo>,
    startnode: NodeHash,
    path: MPath,
) -> BoxStream<(NodeHash, Parents, NodeHash, Option<(MPath, NodeHash)>), Error> {
    if startnode == NULL_HASH {
        return stream::empty().boxify();
    }
    let mut startstate = VecDeque::new();
    startstate.push_back(startnode);
    let seen_nodes: HashSet<_> = [startnode].iter().cloned().collect();

    stream::unfold(
        (startstate, seen_nodes),
        move |cur_data: (VecDeque<NodeHash>, HashSet<NodeHash>)| {
            let (mut nodes, mut seen_nodes) = cur_data;
            let node = nodes.pop_front()?;

            let parents = repo.get_parents(&node);
            let copy = repo.get_file_copy(&node);

            let linknode = RepoPath::file(path.clone()).into_future().and_then({
                let repo = repo.clone();
                move |path| repo.get_linknode(path, &node)
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

fn create_remotefilelog_blob(
    repo: Arc<BlobRepo>,
    node: NodeHash,
    path: MPath,
) -> BoxFuture<Bytes, Error> {
    // raw_content includes copy information
    let raw_content_bytes = repo.get_file_content(&node).and_then(move |raw_content| {
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
    });

    let file_history_bytes = get_file_history(repo, node, path)
        .collect()
        .and_then(|history| {
            let approximate_history_entry_size = 81;
            let mut writer = Cursor::new(Vec::with_capacity(
                history.len() * approximate_history_entry_size,
            ));

            for (node, parents, linknode, copy) in history {
                let (p1, p2) = match parents {
                    Parents::None => (NULL_HASH, NULL_HASH),
                    Parents::One(p) => (p, NULL_HASH),
                    Parents::Two(p1, p2) => (p1, p2),
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

                writer.write_all(node.sha1().as_ref())?;
                writer.write_all(p1.sha1().as_ref())?;
                writer.write_all(p2.sha1().as_ref())?;
                writer.write_all(linknode.sha1().as_ref())?;
                if let Some(copied_from) = copied_from {
                    writer.write_all(&copied_from.to_vec())?;
                }

                write!(writer, "\0")?;
            }
            Ok(writer.into_inner())
        });

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
