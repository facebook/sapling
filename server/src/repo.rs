// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! State for a single source control Repo

use std::collections::{HashMap, HashSet};
use std::fmt::{self, Debug};
use std::io::{BufRead, Cursor};
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use failure::err_msg;
use futures::{future, stream, Async, Future, IntoFuture, Poll, Stream};
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use tokio_core::reactor::Remote;
use tokio_io::AsyncRead;

use slog::Logger;

use mercurial;
use mercurial_bundles::{parts, Bundle2EncodeBuilder};
use mercurial_bundles::bundle2::{self, Bundle2Stream, StreamEvent};
use mercurial_types::{percent_encode, BlobNode, Changeset, Entry, MPath, ManifestId, NodeHash,
                      Parents, RepoPath, Type, NULL_HASH};
use mercurial_types::manifest_utils::{changed_entry_stream, EntryStatus};
use metaconfig::repoconfig::RepoType;

use hgproto::{self, GetbundleArgs, GettreepackArgs, HgCommandRes, HgCommands};

use blobrepo::BlobRepo;

use errors::*;

use repoinfo::RepoGenCache;
use revset::{AncestorsNodeStream, IntersectNodeStream, NodeStream, SetDifferenceNodeStream,
             SingleNodeHash, UnionNodeStream};

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
    ]
}

fn bundle2caps() -> String {
    let caps = hashmap! {
        "HG20" => vec![],
        "listkeys" => vec![],
        "changegroup" => vec!["02"],
    };

    let mut encodedcaps = vec![];

    for (key, value) in &caps {
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
                move |node| hgrepo.get_changeset_by_nodeid(&node)
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

        impl Stream for ParentStream<BoxFuture<Box<Changeset>, hgproto::Error>> {
            type Item = NodeHash;
            type Error = hgproto::Error;

            fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
                if self.n == self.bottom || self.n == NULL_HASH {
                    return Ok(Async::Ready(None));
                }

                self.wait_cs = self.wait_cs
                    .take()
                    .or_else(|| Some(self.repo.hgrepo.get_changeset_by_nodeid(&self.n)));
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
    fn unbundle<R>(
        &self,
        heads: Vec<String>,
        stream: Bundle2Stream<'static, R>,
    ) -> HgCommandRes<bundle2::Remainder<R>>
    where
        R: AsyncRead + BufRead + 'static + Send,
    {
        info!(self.logger, "unbundle heads {:?}", heads);
        stream
            .filter_map(|event| match event {
                StreamEvent::Done(remainder) => Some(remainder),
                StreamEvent::Next(_) => None,
            })
            .into_future()
            .map(|(remainder, _)| remainder.expect("No remainder left"))
            .map_err(|(err, _)| err)
            .boxify()
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

        if params.mfnodes.len() != 1 || params.basemfnodes.len() != 1 {
            // For now, just 1 mfnode and 1 basenode
            return Err(err_msg("only one mfnode and one basemfnode is supported"))
                .into_future()
                .boxify();
        }
        let manifest_id = params.mfnodes.get(0).unwrap();
        let basemfnode = params.basemfnodes.get(0).unwrap();

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

        let manifest = self.repo.hgrepo.get_manifest_by_nodeid(manifest_id);
        let basemanifest = self.repo.hgrepo.get_manifest_by_nodeid(basemfnode);

        let changed_entries = manifest
            .join(basemanifest)
            .map(|(mf, basemf)| changed_entry_stream(mf, basemf, MPath::empty()))
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
                let hgrepo = self.repo.hgrepo.clone();
                move |(entry, path)| fetch_linknode(hgrepo.clone(), entry, path)
            })
            .map(|(entry, linknode, basepath)| (entry, linknode, basepath));

        // Append root manifest
        let root_entry_stream = Ok(self.repo
            .hgrepo
            .get_root_entry(&ManifestId::new(*manifest_id)))
            .into_future()
            .and_then({
                let hgrepo = self.repo.hgrepo.clone();
                move |entry| fetch_linknode(hgrepo.clone(), entry, MPath::empty())
            })
            .map(|(entry, linknode, basepath)| stream::once(Ok((entry, linknode, basepath))))
            .flatten_stream();

        parts::treepack_part(changed_entries.chain(root_entry_stream))
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
