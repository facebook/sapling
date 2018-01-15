// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashSet;
use std::collections::hash_map::{Entry, HashMap};
use std::fmt::{self, Display};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use futures::{Async, Future, IntoFuture, Poll, Stream};
use futures::future;
use futures::stream;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use asyncmemo::{Asyncmemo, Filler};
use bookmarks::Bookmarks;
use mercurial_types::{fncache_fsencode, simple_fsencode, BlobNode, MPath, MPathElement, NodeHash,
                      RepoPath, NULL_HASH};
use stockbookmarks::StockBookmarks;
use storage_types::Version;

pub use changeset::RevlogChangeset;
use errors::*;
pub use manifest::RevlogManifest;
use revlog::{self, Revlog, RevlogIter};

type FutureResult<T> = future::FutureResult<T, Error>;

const DEFAULT_LOGS_CAPACITY: usize = 1000000;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum Required {
    Store,
    Fncache,
    Dotencode,
    Generaldelta,
    Treemanifest,
    Manifestv2,
    Usefncache,
    Revlogv1,
    Largefiles,
    Lz4revlog,
    SqlDirstate,
    HgSql,
    TreeDirstate,
}

impl Display for Required {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use self::Required::*;

        let s = match self {
            &Store => "store",
            &Fncache => "fncache",
            &Dotencode => "dotencode",
            &Generaldelta => "generaldelta",
            &Treemanifest => "treemanifest",
            &Manifestv2 => "manifestv2",
            &Usefncache => "usefncache",
            &Revlogv1 => "revlogv1",
            &Largefiles => "largefiles",
            &Lz4revlog => "lz4revlog",
            &SqlDirstate => "sqldirstate",
            &HgSql => "hgsql",
            &TreeDirstate => "treedirstate",
        };
        write!(fmt, "{}", s)
    }
}

impl FromStr for Required {
    type Err = Error;

    fn from_str(s: &str) -> Result<Required> {
        use self::Required::*;

        match s {
            "store" => Ok(Store),
            "fncache" => Ok(Fncache),
            "dotencode" => Ok(Dotencode),
            "generaldelta" => Ok(Generaldelta),
            "treemanifest" => Ok(Treemanifest),
            "manifestv2" => Ok(Manifestv2),
            "usefncache" => Ok(Usefncache),
            "revlogv1" => Ok(Revlogv1),
            "largefiles" => Ok(Largefiles),
            "lz4revlog" => Ok(Lz4revlog),
            "sqldirstate" => Ok(SqlDirstate),
            "hgsql" => Ok(HgSql),
            "treedirstate" => Ok(TreeDirstate),
            unk => Err(ErrorKind::UnknownReq(unk.into()).into()),
        }
    }
}

/// Representation of a whole Mercurial repo
///
/// `Repo` represents a whole repo: ie, the complete history of a set of files.
/// It consists of an overall history in the form of a DAG of revisions, or changesets.
/// This DAG will typically have a single initial version (though it could have more if
/// histories are merged) and one or more heads, which are revisions which have no children.
///
/// Some revisions can be explicitly named with "bookmarks", and they're often heads as well.
///
/// At the filesystem level, the repo consists of:
///  - the changelog: .hg/store/00changelog.[di]
///  - the manifest: .hg/store/00manifest.[di]
///  - the tree manifests: .hg/store/00manifesttree.[di] and .hg/store/meta/.../00manifest.i
///  - per-file histories: .hg/store/data/.../<file>.[di]
#[derive(Debug, Clone)]
pub struct RevlogRepo {
    basepath: PathBuf,               // path to .hg directory
    requirements: HashSet<Required>, // requirements
    changelog: Revlog,               // changes
    manifest: Revlog,                // manifest
    inner: Arc<RwLock<RevlogInner>>, // Inner parts
    inmemory_logs_capacity: usize,   // Limit on the number of filelogs and tree revlogs in memory.
                                     // Note: there can be 2 * inmemory_logs_capacity revlogs in
                                     // memory in total: half for filelogs and half for revlogs.
}

pub struct RevlogRepoOptions {
    pub inmemory_logs_capacity: usize,
}

#[derive(Debug)]
struct RevlogInner {
    filelogcache: HashMap<MPath, Revlog>, // filelog cache
    treelogcache: HashMap<MPath, Revlog>,
}

impl PartialEq<Self> for RevlogRepo {
    fn eq(&self, other: &Self) -> bool {
        self.basepath == other.basepath && self.requirements == other.requirements
            && Arc::ptr_eq(&self.inner, &other.inner)
    }
}
impl Eq for RevlogRepo {}

impl RevlogRepo {
    pub fn open<P: Into<PathBuf>>(base: P) -> Result<RevlogRepo> {
        let options = RevlogRepoOptions {
            inmemory_logs_capacity: DEFAULT_LOGS_CAPACITY,
        };
        RevlogRepo::open_with_options(base, options)
    }

    pub fn open_with_options<P: Into<PathBuf>>(
        base: P,
        options: RevlogRepoOptions,
    ) -> Result<RevlogRepo> {
        let base = base.into();
        let store = base.as_path().join("store");

        let changelog = Revlog::from_idx_data(store.join("00changelog.i"), None as Option<String>)?;
        let tree_manifest_path = store.join("00manifesttree.i");
        let manifest = if tree_manifest_path.exists() {
            Revlog::from_idx_data(tree_manifest_path, None as Option<String>)?
        } else {
            // Fallback to flat manifest
            Revlog::from_idx_data(store.join("00manifest.i"), None as Option<String>)?
        };

        let mut req = HashSet::new();
        let file = fs::File::open(base.join("requires")).context("Can't open `requires`")?;
        for line in BufReader::new(file).lines() {
            req.insert(line.context("Line read failed")?.parse()?);
        }

        Ok(RevlogRepo {
            basepath: base.into(),
            requirements: req,
            changelog: changelog,
            manifest: manifest,
            inner: Arc::new(RwLock::new(RevlogInner {
                filelogcache: HashMap::new(),
                treelogcache: HashMap::new(),
            })),
            inmemory_logs_capacity: options.inmemory_logs_capacity,
        })
    }

    pub fn get_heads(&self) -> BoxStream<NodeHash, Error> {
        match self.changelog.get_heads() {
            Err(e) => stream::once(Err(e)).boxify(),
            Ok(set) => stream::iter_ok(set.into_iter()).boxify(),
        }
    }

    #[inline]
    pub fn get_changelog(&self) -> &Revlog {
        &self.changelog
    }

    pub fn changeset_exists(&self, nodeid: &NodeHash) -> FutureResult<bool> {
        Ok(self.changelog.get_idx_by_nodeid(nodeid).is_ok()).into_future()
    }

    pub fn get_changeset_blob_by_nodeid(&self, nodeid: &NodeHash) -> FutureResult<BlobNode> {
        self.changelog
            .get_idx_by_nodeid(nodeid)
            .and_then(|idx| self.changelog.get_rev(idx))
            .into_future()
    }

    pub fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<RevlogChangeset, Error> {
        // TODO: (jsgf) T17932873 distinguish between not existing vs some other error
        self.get_changeset_blob_by_nodeid(nodeid)
            .and_then(|rev| RevlogChangeset::new(rev))
            .boxify()
    }

    pub fn get_changelog_revlog_entry_by_nodeid(
        &self,
        nodeid: &NodeHash,
    ) -> FutureResult<revlog::Entry> {
        self.changelog.get_entry_by_nodeid(nodeid).into_future()
    }

    pub fn get_manifest_blob_by_nodeid(&self, nodeid: &NodeHash) -> FutureResult<BlobNode> {
        // It's possible that commit has null pointer to manifest hash.
        // In that case we want to return empty blobnode
        let blobnode = if nodeid == &NULL_HASH {
            Ok(BlobNode::new(vec![], None, None))
        } else {
            self.manifest
                .get_idx_by_nodeid(nodeid)
                .and_then(|idx| self.manifest.get_rev(idx))
        };
        blobnode.into_future()
    }

    pub fn get_tree_manifest_blob_by_nodeid(
        &self,
        nodeid: &NodeHash,
        path: &MPath,
    ) -> FutureResult<BlobNode> {
        self.get_tree_revlog(path)
            .and_then(|tree_revlog| {
                let idx = tree_revlog.get_idx_by_nodeid(nodeid)?;
                tree_revlog.get_rev(idx)
            })
            .into_future()
    }

    pub fn get_manifest_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<RevlogManifest, Error> {
        // TODO: (jsgf) T17932873 distinguish between not existing vs some other error
        let repo = self.clone();
        self.get_manifest_blob_by_nodeid(nodeid)
            .and_then(|rev| RevlogManifest::new(repo, rev))
            .boxify()
    }

    pub fn get_requirements(&self) -> &HashSet<Required> {
        &self.requirements
    }

    pub fn get_path_revlog(&self, path: &RepoPath) -> Result<Revlog> {
        match *path {
            // TODO avoid creating a new MPath here
            RepoPath::RootPath => self.get_tree_revlog(&MPath::empty()),
            RepoPath::DirectoryPath(ref path) => self.get_tree_revlog(path),
            RepoPath::FilePath(ref path) => self.get_file_revlog(path),
        }
    }

    pub fn get_tree_revlog(&self, path: &MPath) -> Result<Revlog> {
        {
            let inner = self.inner.read().expect("poisoned lock");
            let res = inner.treelogcache.get(path);
            if res.is_some() {
                return Ok(res.unwrap().clone());
            }
        }
        let mut inner = self.inner.write().expect("poisoned lock");

        // We may have memory issues if we are keeping too many revlogs in memory.
        // Let's clear them when we have too much
        if inner.treelogcache.len() > self.inmemory_logs_capacity {
            inner.treelogcache.clear();
        }
        match inner.treelogcache.entry(path.clone()) {
            Entry::Occupied(log) => Ok(log.get().clone()),

            Entry::Vacant(missing) => {
                let idxpath = self.get_tree_log_idx_path(path);
                let datapath = self.get_tree_log_data_path(path);
                let revlog = Revlog::from_idx_data(idxpath, Some(datapath))?;
                Ok(missing.insert(revlog).clone())
            }
        }
    }

    pub fn get_file_revlog(&self, path: &MPath) -> Result<Revlog> {
        {
            let inner = self.inner.read().expect("poisoned lock");
            let res = inner.filelogcache.get(path);
            if res.is_some() {
                return Ok(res.unwrap().clone());
            }
        }
        let mut inner = self.inner.write().expect("poisoned lock");

        // We may have memory issues if we are keeping too many revlogs in memory.
        // Let's clear them when we have too much
        if inner.filelogcache.len() > self.inmemory_logs_capacity {
            inner.filelogcache.clear();
        }
        match inner.filelogcache.entry(path.clone()) {
            Entry::Occupied(log) => Ok(log.get().clone()),

            Entry::Vacant(missing) => {
                let idxpath = self.get_file_log_idx_path(path);
                let datapath = self.get_file_log_data_path(path);
                let revlog = Revlog::from_idx_data(idxpath, Some(datapath))?;
                Ok(missing.insert(revlog).clone())
            }
        }
    }

    fn get_tree_log_idx_path(&self, path: &MPath) -> PathBuf {
        self.get_tree_log_path(path, "00manifest.i".as_bytes())
    }

    fn get_tree_log_data_path(&self, path: &MPath) -> PathBuf {
        self.get_tree_log_path(path, "00manifest.d".as_bytes())
    }

    fn get_file_log_idx_path(&self, path: &MPath) -> PathBuf {
        self.get_file_log_path(path, ".i")
    }

    fn get_file_log_data_path(&self, path: &MPath) -> PathBuf {
        self.get_file_log_path(path, ".d")
    }

    fn get_tree_log_path<E: AsRef<[u8]>>(&self, path: &MPath, filename: E) -> PathBuf {
        let filename = filename.as_ref();
        let mut elements: Vec<MPathElement> = vec![MPathElement::new(Vec::from("meta".as_bytes()))];
        elements.extend(path.into_iter().cloned());
        elements.push(MPathElement::new(Vec::from(filename)));
        self.basepath
            .join("store")
            .join(self.fsencode_path(&elements))
    }

    fn get_file_log_path<E: AsRef<[u8]>>(&self, path: &MPath, extension: E) -> PathBuf {
        let extension = extension.as_ref();
        let mut elements: Vec<MPathElement> = vec![MPathElement::new(Vec::from("data".as_bytes()))];
        elements.extend(path.into_iter().cloned());
        if let Some(last) = elements.last_mut() {
            last.extend(extension);
        }
        self.basepath
            .join("store")
            .join(self.fsencode_path(&elements))
    }

    fn fsencode_path(&self, elements: &Vec<MPathElement>) -> PathBuf {
        // Mercurial has a complicated logic of path encoding.
        // Code below matches core Mercurial logic from the commit
        // 75013952d8d9608f73cd45f68405fbd6ec112bf2 from file mercurial/store.py from the function
        // store(). The only caveat is that basicstore is not yet implemented
        if self.requirements.contains(&Required::Store) {
            if self.requirements.contains(&Required::Fncache) {
                let dotencode = self.requirements.contains(&Required::Dotencode);
                fncache_fsencode(&elements, dotencode)
            } else {
                simple_fsencode(&elements)
            }
        } else {
            unimplemented!();
        }
    }

    pub fn bookmarks(&self) -> Result<StockBookmarks> {
        Ok(StockBookmarks::read(self.basepath.clone())?)
    }

    pub fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Error> {
        match self.bookmarks() {
            Ok(b) => b.get(key).boxify(),
            Err(e) => future::err(e).boxify(),
        }
    }

    pub fn changesets(&self) -> ChangesetStream {
        ChangesetStream::new(&self.changelog)
    }
}

pub struct ChangesetBlobFiller(RevlogRepo);
impl ChangesetBlobFiller {
    pub fn new(revlog: &RevlogRepo) -> Self {
        ChangesetBlobFiller(revlog.clone())
    }
}

impl Filler for ChangesetBlobFiller {
    type Key = NodeHash;
    type Value = BoxFuture<Arc<BlobNode>, Error>;

    fn fill(&self, _: &Asyncmemo<Self>, key: &Self::Key) -> Self::Value {
        self.0
            .get_changeset_blob_by_nodeid(&key)
            .map(Arc::new)
            .boxify()
    }
}

pub struct ManifestBlobFiller(RevlogRepo);
impl ManifestBlobFiller {
    pub fn new(revlog: &RevlogRepo) -> Self {
        ManifestBlobFiller(revlog.clone())
    }
}

impl Filler for ManifestBlobFiller {
    type Key = NodeHash;
    type Value = BoxFuture<Arc<BlobNode>, Error>;

    fn fill(&self, _: &Asyncmemo<Self>, key: &Self::Key) -> Self::Value {
        self.0
            .get_manifest_blob_by_nodeid(&key)
            .map(Arc::new)
            .boxify()
    }
}

pub struct ChangesetStream(RevlogIter);

impl ChangesetStream {
    fn new(changelog: &Revlog) -> Self {
        ChangesetStream(changelog.into_iter())
    }
}

impl Stream for ChangesetStream {
    type Item = NodeHash;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<NodeHash>, Error> {
        match self.0.next() {
            Some((_, e)) => Ok(Async::Ready(Some(e.nodeid))),
            None => Ok(Async::Ready(None)),
        }
    }
}
