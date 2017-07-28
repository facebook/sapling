// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io::{BufRead, BufReader};
use std::str::FromStr;
use std::fs;
use std::fmt::{self, Display};
use std::path::PathBuf;
use std::collections::HashSet;
use std::collections::hash_map::{Entry, HashMap};
use std::sync::{Arc, Mutex};

use futures::{Async, Future, IntoFuture, Poll, Stream};
use futures::future::{self, BoxFuture};
use futures::stream::{self, BoxStream};

use asyncmemo::Filler;
use mercurial_types::{BlobNode, Changeset, Manifest, NodeHash, Path, Repo};
use stockbookmarks::StockBookmarks;

use errors::*;
use revlog::{Revlog, RevlogIter};
pub use changeset::RevlogChangeset;
pub use manifest::RevlogManifest;

type FutureResult<T> = future::FutureResult<T, Error>;

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
///  - per-file histories: .hg/store/data/.../<file>.[di]
#[derive(Debug, Clone)]
pub struct RevlogRepo {
    basepath: PathBuf, // path to .hg directory
    requirements: HashSet<Required>, // requirements
    inner: Arc<Mutex<RevlogInner>>, // Inner parts
}

#[derive(Debug)]
struct RevlogInner {
    changelog: Revlog, // changes
    manifest: Revlog, // manifest
    filelogcache: HashMap<Path, Revlog>, // filelog cache
}

impl PartialEq<Self> for RevlogRepo {
    fn eq(&self, other: &Self) -> bool {
        self.basepath == other.basepath && self.requirements == other.requirements &&
            Arc::ptr_eq(&self.inner, &other.inner)
    }
}
impl Eq for RevlogRepo {}

impl RevlogRepo {
    pub fn open<P: Into<PathBuf>>(base: P) -> Result<RevlogRepo> {
        let base = base.into();
        let store = base.as_path().join("store");

        let changelog = Revlog::from_idx_data(store.join("00changelog.i"), None as Option<String>)?;
        let manifest = Revlog::from_idx_data(store.join("00manifest.i"), None as Option<String>)?;

        let mut req = HashSet::new();
        let file = fs::File::open(base.join("requires"))
            .chain_err(|| "Can't open `requires`")?;
        for line in BufReader::new(file).lines() {
            req.insert(line.chain_err(|| "Line read failed")?.parse()?);
        }

        Ok(RevlogRepo {
            basepath: base.into(),
            requirements: req,
            inner: Arc::new(Mutex::new(RevlogInner {
                changelog: changelog,
                manifest: manifest,
                filelogcache: HashMap::new(),
            })),
        })
    }

    pub fn get_heads(&self) -> BoxStream<NodeHash, Error> {
        let mut inner = self.inner.lock().expect("poisoned lock");
        match inner.changelog.get_heads() {
            Err(e) => stream::once(Err(e)).boxed(),
            Ok(set) => stream::iter(set.into_iter().map(|e| Ok(e))).boxed(),
        }
    }

    pub fn changeset_exists(&self, nodeid: &NodeHash) -> FutureResult<bool> {
        let inner = self.inner.lock().expect("poisoned lock");

        Ok(inner.changelog.get_idx_by_nodeid(nodeid).is_ok()).into_future()
    }

    pub fn get_changeset_blob_by_nodeid(&self, nodeid: &NodeHash) -> FutureResult<BlobNode> {
        let inner = self.inner.lock().expect("poisoned lock");

        inner
            .changelog
            .get_idx_by_nodeid(nodeid)
            .and_then(|idx| inner.changelog.get_rev(idx))
            .into_future()
    }

    pub fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<RevlogChangeset, Error> {
        // TODO: (jsgf) T17932873 distinguish between not existing vs some other error
        self.get_changeset_blob_by_nodeid(nodeid)
            .and_then(|rev| RevlogChangeset::new(rev))
            .boxed()
    }

    pub fn get_manifest_blob_by_nodeid(&self, nodeid: &NodeHash) -> FutureResult<BlobNode> {
        let inner = self.inner.lock().expect("poisoned lock");

        inner
            .manifest
            .get_idx_by_nodeid(nodeid)
            .and_then(|idx| inner.manifest.get_rev(idx))
            .into_future()
    }

    pub fn get_manifest_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<RevlogManifest, Error> {
        // TODO: (jsgf) T17932873 distinguish between not existing vs some other error
        let repo = self.clone();
        self.get_manifest_blob_by_nodeid(nodeid)
            .and_then(|rev| RevlogManifest::new(repo, rev))
            .boxed()
    }

    pub fn get_requirements(&self) -> &HashSet<Required> {
        &self.requirements
    }

    pub fn get_file_revlog(&self, path: &Path) -> Result<Revlog> {
        let mut inner = self.inner.lock().expect("poisoned lock");

        match inner.filelogcache.entry(path.clone()) {
            Entry::Occupied(log) => Ok(log.get().clone()),

            Entry::Vacant(missing) => {
                let dotencode = self.requirements.contains(&Required::Dotencode);
                let mut path = self.basepath
                    .join("store")
                    .join("data")
                    .join(path.fsencode(dotencode));
                if let Some(ext) = path.extension()
                    .map(|ext| ext.to_string_lossy().into_owned())
                {
                    path.set_extension(format!("{}.i", ext));
                } else {
                    path.set_extension("i");
                }

                let revlog = Revlog::from_idx_data(path, None as Option<String>)?;
                Ok(missing.insert(revlog).clone())
            }
        }
    }

    pub fn bookmarks(&self) -> Result<StockBookmarks> {
        Ok(StockBookmarks::read(self.basepath.clone())?)
    }

    pub fn changesets(&self) -> ChangesetStream {
        let inner = self.inner.lock().expect("poisoned lock");
        ChangesetStream::new(&inner.changelog)
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

    fn fill(&self, key: &Self::Key) -> Self::Value {
        self.0
            .get_changeset_blob_by_nodeid(&key)
            .map(Arc::new)
            .boxed()
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

    fn fill(&self, key: &Self::Key) -> Self::Value {
        self.0
            .get_manifest_blob_by_nodeid(&key)
            .map(Arc::new)
            .boxed()
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

impl Repo for RevlogRepo {
    type Error = Error;

    fn get_heads(&self) -> BoxStream<NodeHash, Self::Error> {
        self.get_heads().boxed()
    }

    fn get_changesets(&self) -> BoxStream<NodeHash, Self::Error> {
        self.changesets().boxed()
    }

    fn changeset_exists(&self, nodeid: &NodeHash) -> BoxFuture<bool, Self::Error> {
        RevlogRepo::changeset_exists(self, nodeid).boxed()
    }

    fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Self::Error> {
        RevlogRepo::get_changeset_by_nodeid(self, nodeid)
            .map(|cs| cs.boxed())
            .boxed()
    }

    fn get_manifest_by_nodeid(
        &self,
        nodeid: &NodeHash,
    ) -> BoxFuture<Box<Manifest<Error = Self::Error>>, Self::Error> {
        RevlogRepo::get_manifest_by_nodeid(self, nodeid)
            .map(|m| m.boxed())
            .boxed()
    }
}
