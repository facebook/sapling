/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    collections::HashMap,
    convert::TryInto,
    fs::File,
    io::{ErrorKind, Read, Write},
    iter,
    path::{Path, PathBuf},
    str::{self, FromStr},
    sync::Arc,
};

use anyhow::{bail, ensure, Result};
use bytes::{Bytes, BytesMut};
use futures::stream::{iter, StreamExt};
use parking_lot::{Mutex, RwLock};
use reqwest::{Client, IntoUrl, Method, RequestBuilder, Url};
use serde_derive::{Deserialize, Serialize};
use tokio::runtime::Runtime;

use configparser::{
    config::ConfigSet,
    hg::{ByteCount, ConfigSetHgExt},
};
use indexedlog::log::IndexOutput;
use lfs_protocol::{
    ObjectStatus, Operation, RequestBatch, RequestObject, ResponseBatch, Sha256 as LfsSha256,
};
use mincode::{deserialize, serialize};
use types::{HgId, Key, RepoPath, Sha256};
use util::path::create_dir;

use crate::{
    datastore::{
        strip_metadata, ContentDataStore, ContentMetadata, Delta, HgIdDataStore,
        HgIdMutableDeltaStore, Metadata, RemoteDataStore,
    },
    historystore::{HgIdMutableHistoryStore, RemoteHistoryStore},
    indexedlogutil::{Store, StoreOpenOptions},
    localstore::LocalStore,
    remotestore::HgIdRemoteStore,
    types::{ContentHash, StoreKey},
    uniondatastore::UnionHgIdDataStore,
    util::{get_lfs_blobs_path, get_lfs_pointers_path, get_str_config},
};

/// The `LfsPointersStore` holds the mapping between a `HgId` and the content hash (sha256) of the LFS blob.
struct LfsPointersStore(Store);

/// The `LfsBlobsStore` holds the actual blobs. Lookup is done via the content hash (sha256) of the
/// blob.
#[derive(Clone)]
struct LfsBlobsStore(PathBuf, bool);

#[derive(Clone)]
struct HttpLfsRemote {
    url: Url,
    user_agent: String,
    concurrent_fetches: usize,
    client: Client,
    rt: Arc<Mutex<Runtime>>,
}

#[derive(Clone)]
enum LfsRemoteInner {
    Http(HttpLfsRemote),
    File(LfsBlobsStore),
}

#[derive(Clone)]
pub struct LfsRemote {
    local: Arc<LfsStore>,
    remote: LfsRemoteInner,
}

/// Main LFS store to be used within the `ContentStore`.
///
/// The on-disk layout of the LFS store is 2 parts:
///  - A pointers store that holds a mapping between a `HgId` and the content hash of a blob. This
///    store is under the top-level directory "pointers".
///  - A blob store that holds the actual data. This store is under the top-level directory
///    "objects". Under that directory, the string representation of its content hash is used to
///    find the file storing the data: <2-digits hex>/<62-digits hex>
pub struct LfsStore {
    pointers: RwLock<LfsPointersStore>,
    blobs: LfsBlobsStore,
}

/// When a blob is added to the `LfsMultiplexer`, is will either be written to an `LfsStore`, or to
/// a regular `HgIdMutableDeltaStore`. The choice is made based on whether the blob is larger than a
/// user defined threshold, or on whether the blob being added represents an LFS pointer.
pub struct LfsMultiplexer {
    lfs: Arc<LfsStore>,
    non_lfs: Arc<dyn HgIdMutableDeltaStore>,

    threshold: usize,

    union: UnionHgIdDataStore<Arc<dyn HgIdMutableDeltaStore>>,
}

#[derive(
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Copy,
    Clone,
    Hash,
    Serialize,
    Deserialize
)]
enum ContentHashType {
    Sha256,
}

/// On-disk format of an LFS pointer. This is directly serialized with the mincode encoding, and
/// thus changes to this structure must be done in a backward and forward compatible fashion.
#[derive(Serialize, Deserialize)]
struct LfsPointersEntry {
    hgid: HgId,
    size: u64,
    is_binary: bool,
    copy_from: Option<Key>,
    /// The content_hashes will always contain at least a `ContentHashType::Sha256` entry.
    content_hashes: HashMap<ContentHashType, ContentHash>,
}

impl LfsPointersStore {
    const INDEX_NODE: usize = 0;
    const INDEX_SHA256: usize = 1;

    fn open_options(config: &ConfigSet) -> Result<StoreOpenOptions> {
        let log_size = config.get_or("lfs", "pointersstoresize", || ByteCount::from(40_000_000))?;
        Ok(StoreOpenOptions::new()
            .max_log_count(4)
            .max_bytes_per_log(log_size.value() / 4)
            .index("node", |_| {
                vec![IndexOutput::Reference(0..HgId::len() as u64)]
            })
            .index("sha256", |buf| {
                let pointer = LfsPointersStore::get_from_slice(buf).unwrap();

                // We're guaranteed to have at least a sha256 entry.
                let content_hash = pointer.content_hashes[&ContentHashType::Sha256].clone();
                vec![IndexOutput::Owned(Box::from(
                    content_hash.unwrap_sha256().as_ref(),
                ))]
            }))
    }

    /// Create a local `LfsPointersStore`.
    fn local(path: &Path, config: &ConfigSet) -> Result<Self> {
        let path = get_lfs_pointers_path(path)?;
        Ok(Self(LfsPointersStore::open_options(config)?.local(path)?))
    }

    /// Create a shared `LfsPointersStore`.
    fn shared(path: &Path, config: &ConfigSet) -> Result<Self> {
        let path = get_lfs_pointers_path(path)?;
        Ok(Self(LfsPointersStore::open_options(config)?.shared(path)?))
    }

    /// Read an entry from the slice and deserialize it.
    fn get_from_slice(data: &[u8]) -> Result<LfsPointersEntry> {
        Ok(deserialize(data)?)
    }

    /// Find the pointer corresponding to the passed in `StoreKey`.
    fn entry(&self, key: &StoreKey) -> Result<Option<LfsPointersEntry>> {
        let mut iter = match key {
            StoreKey::HgId(key) => self.0.lookup(Self::INDEX_NODE, key.hgid)?,
            StoreKey::Content(hash) => match hash {
                ContentHash::Sha256(hash) => self.0.lookup(Self::INDEX_SHA256, hash)?,
            },
        };

        let buf = match iter.next() {
            None => return Ok(None),
            Some(buf) => buf?,
        };

        Self::get_from_slice(buf).map(Some)
    }

    /// Find the pointer corresponding to the passed in `Key`.
    fn get(&self, key: &Key) -> Result<Option<LfsPointersEntry>> {
        self.entry(&StoreKey::from(key))
    }

    fn add(&mut self, entry: LfsPointersEntry) -> Result<()> {
        Ok(self.0.append(serialize(&entry)?)?)
    }
}

impl LfsBlobsStore {
    fn local(path: &Path) -> Result<Self> {
        Ok(Self(get_lfs_blobs_path(path)?, true))
    }

    fn shared(path: &Path) -> Result<Self> {
        Ok(Self(get_lfs_blobs_path(path)?, false))
    }

    fn path(&self, hash: &Sha256) -> PathBuf {
        let mut path = self.0.to_path_buf();
        let mut hex = hash.to_hex();

        let second = hex.split_off(2);
        path.push(hex);
        path.push(second);

        path
    }

    /// Read the blob matching the content hash.
    ///
    /// XXX: The blob hash is not validated.
    fn get(&self, hash: &Sha256) -> Result<Option<Bytes>> {
        let path = self.path(hash);

        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(e) => {
                if e.kind() == ErrorKind::NotFound {
                    return Ok(None);
                } else {
                    return Err(e.into());
                }
            }
        };

        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        Ok(Some(Bytes::from(buf)))
    }

    /// Test whether the blob store contains the hash. A file of the correct name is for now
    /// indicating that it exists.
    fn contains(&self, hash: &Sha256) -> bool {
        let path = self.path(hash);
        path.is_file()
    }

    /// Add the blob to the store.
    fn add(&self, hash: &Sha256, blob: Bytes) -> Result<()> {
        let path = self.path(hash);
        create_dir(path.parent().unwrap())?;

        let mut file = File::create(path)?;
        file.write_all(&blob)?;

        if self.1 {
            file.sync_all()?;
        }

        Ok(())
    }
}

impl LfsStore {
    fn new(pointers: LfsPointersStore, blobs: LfsBlobsStore) -> Result<Self> {
        Ok(Self {
            pointers: RwLock::new(pointers),
            blobs,
        })
    }

    /// Create a new local `LfsStore`.
    ///
    /// Local stores will `fsync(2)` data to disk, and will never rotate data out of the store.
    pub fn local(path: impl AsRef<Path>, config: &ConfigSet) -> Result<Self> {
        let path = path.as_ref();
        let pointers = LfsPointersStore::local(path, config)?;
        let blobs = LfsBlobsStore::local(path)?;
        LfsStore::new(pointers, blobs)
    }

    /// Create a new shared `LfsStore`.
    pub fn shared(path: impl AsRef<Path>, config: &ConfigSet) -> Result<Self> {
        let path = path.as_ref();
        let pointers = LfsPointersStore::shared(path, config)?;
        let blobs = LfsBlobsStore::shared(path)?;
        LfsStore::new(pointers, blobs)
    }

    fn blob_impl(&self, key: &StoreKey) -> Result<Option<(LfsPointersEntry, Bytes)>> {
        let pointer = self.pointers.read().entry(key)?;

        match pointer {
            None => Ok(None),
            Some(entry) => match entry.content_hashes.get(&ContentHashType::Sha256) {
                None => Ok(None),
                Some(content_hash) => Ok(self
                    .blobs
                    .get(&content_hash.clone().unwrap_sha256())?
                    .map(|blob| (entry, blob))),
            },
        }
    }
}

impl LocalStore for LfsStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys
            .iter()
            .filter_map(|k| match k {
                StoreKey::HgId(key) => {
                    let entry = self.pointers.read().get(key);
                    match entry {
                        Ok(None) | Err(_) => Some(k.clone()),
                        Ok(Some(entry)) => match entry.content_hashes.get(&ContentHashType::Sha256)
                        {
                            None => None,
                            Some(content_hash) => {
                                let sha256 = content_hash.clone().unwrap_sha256();
                                if self.blobs.contains(&sha256) {
                                    None
                                } else {
                                    Some(StoreKey::from(content_hash))
                                }
                            }
                        },
                    }
                }
                StoreKey::Content(content_hash) => match content_hash {
                    ContentHash::Sha256(hash) => {
                        if self.blobs.contains(&hash) {
                            None
                        } else {
                            Some(k.clone())
                        }
                    }
                },
            })
            .collect())
    }
}

/// When a file was copied, Mercurial expects the blob that the store returns to contain this copy
/// information
fn rebuild_metadata(data: Bytes, entry: &LfsPointersEntry) -> Bytes {
    if let Some(copy_from) = &entry.copy_from {
        let mut ret = BytesMut::new();

        ret.extend_from_slice(&b"\x01\n"[..]);
        ret.extend_from_slice(&b"copy: "[..]);
        ret.extend_from_slice(copy_from.path.as_ref());
        ret.extend_from_slice(&b"\n"[..]);
        ret.extend_from_slice(&b"copyrev: "[..]);
        ret.extend_from_slice(copy_from.hgid.to_hex().as_bytes());
        ret.extend_from_slice(&b"\n"[..]);
        ret.extend_from_slice(&b"\x01\n"[..]);
        ret.extend_from_slice(data.as_ref());
        ret.freeze()
    } else {
        if data.as_ref().starts_with(b"\x01\n") {
            let mut ret = BytesMut::new();
            ret.extend_from_slice(&b"\x01\n\x01\n"[..]);
            ret.extend_from_slice(data.as_ref());
            ret.freeze()
        } else {
            data
        }
    }
}

impl HgIdDataStore for LfsStore {
    fn get(&self, _key: &Key) -> Result<Option<Vec<u8>>> {
        unreachable!()
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        match self.blob_impl(&StoreKey::from(key))? {
            Some((entry, content)) => {
                let content = rebuild_metadata(content, &entry);
                Ok(Some(Delta {
                    data: content,
                    base: None,
                    key: key.clone(),
                }))
            }
            None => Ok(None),
        }
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        Ok(self.get_delta(key)?.map(|delta| vec![delta]))
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        let entry = self.pointers.read().get(key)?;
        if let Some(entry) = entry {
            Ok(Some(Metadata {
                size: Some(entry.size.try_into()?),
                flags: None,
            }))
        } else {
            Ok(None)
        }
    }
}

impl HgIdMutableDeltaStore for LfsStore {
    fn add(&self, delta: &Delta, _metadata: &Metadata) -> Result<()> {
        ensure!(delta.base.is_none(), "Deltas aren't supported.");

        let (data, copy_from) = strip_metadata(&delta.data)?;
        let content_hash = ContentHash::sha256(&data)?;

        match content_hash {
            ContentHash::Sha256(sha256) => self.blobs.add(&sha256, data.clone())?,
        };

        let mut content_hashes = HashMap::new();
        content_hashes.insert(ContentHashType::Sha256, content_hash);

        let entry = LfsPointersEntry {
            hgid: delta.key.hgid.clone(),
            size: data.len().try_into()?,
            is_binary: data.as_ref().contains(&b'\0'),
            copy_from,
            content_hashes,
        };
        self.pointers.write().add(entry)
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        self.pointers.write().0.flush()?;
        Ok(None)
    }
}

impl From<LfsPointersEntry> for ContentMetadata {
    fn from(pointer: LfsPointersEntry) -> Self {
        let hash = pointer.content_hashes[&ContentHashType::Sha256].clone();

        ContentMetadata {
            size: pointer.size as usize,
            hash,
            is_binary: pointer.is_binary,
        }
    }
}

impl ContentDataStore for LfsStore {
    fn blob(&self, key: &StoreKey) -> Result<Option<Bytes>> {
        Ok(self.blob_impl(key)?.map(|(_, blob)| blob))
    }

    fn metadata(&self, key: &StoreKey) -> Result<Option<ContentMetadata>> {
        let pointer = self.pointers.read().entry(key)?;

        Ok(pointer.map(Into::into))
    }
}

impl LfsMultiplexer {
    /// Build an `LfsMultiplexer`. All blobs bigger than `threshold` will be written to the `lfs`
    /// store, the others to the `non_lfs` store.
    pub fn new(
        lfs: Arc<LfsStore>,
        non_lfs: Arc<dyn HgIdMutableDeltaStore>,
        threshold: usize,
    ) -> Self {
        let mut union_store = UnionHgIdDataStore::new();
        union_store.add(non_lfs.clone());
        union_store.add(lfs.clone());

        Self {
            lfs,
            non_lfs,
            union: union_store,
            threshold,
        }
    }
}

impl HgIdDataStore for LfsMultiplexer {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        self.union.get(key)
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        self.union.get_delta(key)
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        self.union.get_delta_chain(key)
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        self.union.get_meta(key)
    }
}

impl LocalStore for LfsMultiplexer {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.union.get_missing(keys)
    }
}

const LFS_POINTER_VERSION: &str = "version https://git-lfs.github.com/spec/v1";
const LFS_POINTER_OID_SHA256: &str = "oid sha256:";
const LFS_POINTER_SIZE: &str = "size ";
const LFS_POINTER_X_HG_COPY: &str = "x-hg-copy ";
const LFS_POINTER_X_HG_COPYREV: &str = "x-hg-copyrev ";
const LFS_POINTER_X_IS_BINARY: &str = "x-is-binary ";

impl LfsPointersEntry {
    /// Attempt to convert the bytes to an LfsPointersEntry, the specification for an LFS pointer
    /// can be found at https://github.com/git-lfs/git-lfs/blob/master/docs/spec.md
    fn from_bytes(data: impl AsRef<[u8]>, hgid: HgId) -> Result<Self> {
        let data = str::from_utf8(data.as_ref())?;
        Ok(LfsPointersEntry::from_str(data, hgid)?)
    }

    /// Parse the text representation of an LFS pointer.
    ///
    /// The specification for an LFS pointer can be found at
    /// https://github.com/git-lfs/git-lfs/blob/master/docs/spec.md
    fn from_str(data: &str, hgid: HgId) -> Result<Self> {
        let lines = data.split_terminator('\n');

        let mut hash = None;
        let mut size = None;
        let mut path = None;
        let mut copy_hgid = None;
        let mut is_binary = true;

        for line in lines {
            if line.starts_with(LFS_POINTER_VERSION) {
                continue;
            } else if line.starts_with(LFS_POINTER_OID_SHA256) {
                let oid = &line[LFS_POINTER_OID_SHA256.len()..];
                hash = Some(oid.parse::<Sha256>()?);
            } else if line.starts_with(LFS_POINTER_SIZE) {
                let stored_size = &line[LFS_POINTER_SIZE.len()..];
                size = Some(stored_size.parse::<usize>()?);
            } else if line.starts_with(LFS_POINTER_X_HG_COPY) {
                path = Some(RepoPath::from_str(&line[LFS_POINTER_X_HG_COPY.len()..])?.to_owned());
            } else if line.starts_with(LFS_POINTER_X_HG_COPYREV) {
                copy_hgid = Some(HgId::from_str(&line[LFS_POINTER_X_HG_COPYREV.len()..])?);
            } else if line.starts_with(LFS_POINTER_X_IS_BINARY) {
                let stored_is_binary = &line[LFS_POINTER_X_IS_BINARY.len()..];
                is_binary = stored_is_binary.parse::<u8>()? == 1;
            } else {
                bail!("unknown metadata: {}", line);
            }
        }

        let hash = if let Some(hash) = hash {
            hash
        } else {
            bail!("no oid stored in pointer");
        };

        let size = if let Some(size) = size {
            size
        } else {
            bail!("no size stored in pointer");
        };

        let copy_from = match (path, copy_hgid) {
            (None, Some(_)) => bail!("missing 'x-hg-copyrev' metadata"),
            (Some(_), None) => bail!("missing 'x-hg-copy' metadata"),

            (None, None) => None,
            (Some(path), Some(copy_hgid)) => Some(Key::new(path, copy_hgid)),
        };

        let mut content_hashes = HashMap::new();
        content_hashes.insert(ContentHashType::Sha256, ContentHash::Sha256(hash));

        Ok(LfsPointersEntry {
            hgid,
            size: size.try_into()?,
            is_binary,
            copy_from,
            content_hashes,
        })
    }
}

impl HgIdMutableDeltaStore for LfsMultiplexer {
    /// Add the blob to the store.
    ///
    /// Depending on whether the blob represents an LFS pointer, or if it is large enough, it will
    /// be added either to the lfs store, or to the non-lfs store.
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        if metadata.is_lfs() {
            // This is an lfs pointer blob. Let's parse it and extract what matters.
            let pointer = LfsPointersEntry::from_bytes(&delta.data, delta.key.hgid.clone())?;
            return self.lfs.pointers.write().add(pointer);
        }

        if delta.data.len() > self.threshold {
            self.lfs.add(delta, &Default::default())
        } else {
            self.non_lfs.add(delta, metadata)
        }
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        self.non_lfs.flush()?;
        self.lfs.flush()?;
        Ok(None)
    }
}

impl LfsRemoteInner {
    fn batch(
        &self,
        objs: &[(Sha256, usize)],
    ) -> Result<Box<dyn Iterator<Item = Result<(Sha256, Bytes)>>>> {
        match self {
            LfsRemoteInner::Http(http) => Ok(Box::new(Self::batch_http(http, objs)?)),
            LfsRemoteInner::File(file) => Ok(Box::new(Self::batch_file(file, objs)?)),
        }
    }

    /// Fetch blobs from the LFS server.
    ///
    /// The protocol is described at: https://github.com/git-lfs/git-lfs/blob/master/docs/api/batch.md
    fn batch_http(
        http: &HttpLfsRemote,
        objs: &[(Sha256, usize)],
    ) -> Result<impl Iterator<Item = Result<(Sha256, Bytes)>>> {
        let objects = objs
            .iter()
            .map(|(oid, size)| RequestObject {
                oid: LfsSha256(oid.into_inner()),
                size: *size as u64,
            })
            .collect::<Vec<_>>();

        let batch = RequestBatch {
            operation: Operation::Download,
            transfers: vec![Default::default()],
            r#ref: None,
            objects,
        };

        let batch_json = serde_json::to_string(&batch)?;

        fn make_request(http: &HttpLfsRemote, method: Method, url: impl IntoUrl) -> RequestBuilder {
            http.client
                .request(method, url)
                .header("Accept", "application/vnd.git-lfs+json")
                .header("Content-Type", "application/vnd.git-lfs+json")
                .header("User-Agent", &http.user_agent)
        }

        let req = make_request(http, Method::POST, http.url.join("objects/batch")?)
            .body(batch_json)
            .send();

        let response = http
            .rt
            .lock()
            .block_on(async { req.await?.error_for_status()?.bytes().await })?;

        let response: ResponseBatch = serde_json::from_slice(response.as_ref())?;

        let mut futures = Vec::new();

        for object in response.objects {
            let oid = object.object.oid;
            let actions = match object.status {
                ObjectStatus::Ok {
                    authenticated: _,
                    actions,
                } => actions,
                ObjectStatus::Err { error: e } => bail!("Couldn't fetch oid {}: {:?}", oid, e),
            };

            if let Some(action) = actions.get(&Operation::Download) {
                let mut req = make_request(http, Method::GET, &action.href.to_string());
                if let Some(header) = action.header.as_ref() {
                    for (key, val) in header {
                        req = req.header(key, val)
                    }
                }

                let fut =
                    async move { Ok((oid, req.send().await?.error_for_status()?.bytes().await?)) };
                futures.push(fut);
            }
        }

        // Request a couple of blobs concurrently.
        let mut stream = iter(futures).buffer_unordered(http.concurrent_fetches);

        let rt = http.rt.clone();
        Ok(iter::from_fn(move || {
            let next = stream.next();
            rt.lock()
                .block_on(async { next.await })
                .map(|res| res.map(|(sha, bytes)| ((&sha.0).into(), bytes)))
        }))
    }

    /// Fetch files from the filesystem.
    ///
    /// The implementation is inefficient and will read all the blobs from the disk before
    /// returning. Since file backed LFS servers are only intended for tests purposes this is an
    /// appropriate solution.
    fn batch_file(
        file: &LfsBlobsStore,
        objs: &[(Sha256, usize)],
    ) -> Result<impl Iterator<Item = Result<(Sha256, Bytes)>>> {
        let ret = objs.iter().filter_map(|(hash, _)| {
            file.get(&hash)
                .transpose()
                .map(|data| data.map(|data| (*hash, data)))
        });

        // Avoid lifetime issues by collecting everything.
        Ok(ret.collect::<Vec<_>>().into_iter())
    }
}

impl LfsRemote {
    pub fn new(store: Arc<LfsStore>, config: &ConfigSet) -> Result<Self> {
        let mut url = get_str_config(config, "lfs", "url")?;
        // A trailing '/' needs to be present so that `Url::join` doesn't remove the reponame
        // present at the end of the config.
        url.push('/');

        let url = Url::parse(&url)?;

        if url.scheme() == "file" {
            let path = url.to_file_path().unwrap();
            create_dir(&path)?;
            let file = LfsBlobsStore::shared(&path)?;
            Ok(Self {
                local: store,
                remote: LfsRemoteInner::File(file),
            })
        } else {
            if !["http", "https"].contains(&url.scheme()) {
                bail!("Unsupported url: {}", url);
            }

            let user_agent = config.get_or("experimental", "lfs.user-agent", || {
                "mercurial/revisionstore".to_string()
            })?;

            let concurrent_fetches = config.get_or("lfs", "concurrentfetches", || 1)?;

            let rt = Arc::new(Mutex::new(Runtime::new()?));
            let client = Client::new();
            Ok(Self {
                local: store,
                remote: LfsRemoteInner::Http(HttpLfsRemote {
                    url,
                    user_agent,
                    concurrent_fetches,
                    client,
                    rt,
                }),
            })
        }
    }

    fn batch(
        &self,
        objs: &[(Sha256, usize)],
    ) -> Result<impl Iterator<Item = Result<(Sha256, Bytes)>>> {
        self.remote.batch(objs)
    }
}

impl HgIdRemoteStore for LfsRemote {
    fn datastore(&self, store: Arc<dyn HgIdMutableDeltaStore>) -> Arc<dyn RemoteDataStore> {
        Arc::new(LfsRemoteStore {
            store,
            remote: self.clone(),
        })
    }

    fn historystore(
        &self,
        _store: Arc<dyn HgIdMutableHistoryStore>,
    ) -> Arc<dyn RemoteHistoryStore> {
        unreachable!()
    }
}

struct LfsRemoteStore {
    store: Arc<dyn HgIdMutableDeltaStore>,
    remote: LfsRemote,
}

impl RemoteDataStore for LfsRemoteStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        let objs = keys
            .iter()
            .map(|k| {
                if let Some(pointer) = self.remote.local.pointers.read().entry(k)? {
                    match pointer.content_hashes.get(&ContentHashType::Sha256) {
                        None => Ok(None),
                        Some(content_hash) => Ok(Some((
                            content_hash.clone().unwrap_sha256(),
                            pointer.size.try_into()?,
                        ))),
                    }
                } else {
                    Ok(None)
                }
            })
            .filter_map(|res| res.transpose())
            .collect::<Result<Vec<_>>>()?;

        for response in self.remote.batch(&objs)? {
            let (sha256, content) = response?;
            self.remote.local.blobs.add(&sha256, content)?;
        }

        Ok(())
    }
}

impl HgIdDataStore for LfsRemoteStore {
    fn get(&self, _key: &Key) -> Result<Option<Vec<u8>>> {
        unreachable!();
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        match self.prefetch(&[StoreKey::from(key)]) {
            Ok(()) => self.store.get_delta(key),
            Err(_) => Ok(None),
        }
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        match self.prefetch(&[StoreKey::from(key)]) {
            Ok(()) => self.store.get_delta_chain(key),
            Err(_) => Ok(None),
        }
    }

    fn get_meta(&self, _key: &Key) -> Result<Option<Metadata>> {
        unreachable!();
    }
}

impl LocalStore for LfsRemoteStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::str::FromStr;

    use quickcheck::quickcheck;
    use tempfile::TempDir;

    use types::testutil::*;

    use crate::{indexedlogdatastore::IndexedLogHgIdDataStore, testutil::make_lfs_config};

    #[test]
    fn test_new_shared() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir);
        let _ = LfsStore::shared(&dir, &config)?;

        let mut lfs_dir = dir.as_ref().to_owned();
        lfs_dir.push("lfs");
        lfs_dir.push("objects");
        assert!(lfs_dir.is_dir());

        Ok(())
    }

    #[test]
    fn test_new_local() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir);
        let _ = LfsStore::local(&dir, &config)?;

        let mut lfs_dir = dir.as_ref().to_owned();
        lfs_dir.push("lfs");
        lfs_dir.push("objects");
        assert!(lfs_dir.is_dir());

        Ok(())
    }

    #[test]
    fn test_add() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir);
        let store = LfsStore::shared(&dir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };

        store.add(&delta, &Default::default())?;

        let mut lfs_dir = dir.as_ref().to_owned();
        lfs_dir.push("lfs");
        lfs_dir.push("objects");

        lfs_dir.push("9f");
        assert!(lfs_dir.is_dir());

        lfs_dir.push("64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a");
        assert!(lfs_dir.is_file());

        let mut content = Vec::new();
        File::open(&lfs_dir)?.read_to_end(&mut content)?;

        assert_eq!(Bytes::from(content), delta.data);

        Ok(())
    }

    #[test]
    fn test_add_get_missing() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir);
        let store = LfsStore::shared(&dir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };

        assert_eq!(
            store.get_missing(&[StoreKey::from(&k1)])?,
            vec![StoreKey::from(&k1)]
        );
        store.add(&delta, &Default::default())?;
        assert_eq!(store.get_missing(&[StoreKey::from(k1)])?, vec![]);

        Ok(())
    }

    #[test]
    fn test_add_get() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir);
        let store = LfsStore::shared(&dir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };

        store.add(&delta, &Default::default())?;
        let get_delta = store.get_delta(&k1)?;
        assert_eq!(Some(delta), get_delta);

        Ok(())
    }

    quickcheck! {
        fn metadata_strip_rebuild(data: Vec<u8>, copy_from: Option<Key>) -> Result<bool> {
            let data = Bytes::from(data);

            let mut content_hashes = HashMap::new();
            content_hashes.insert(ContentHashType::Sha256, ContentHash::sha256(&data)?);

            let pointer = LfsPointersEntry {
                hgid: hgid("1234"),
                size: data.len().try_into()?,
                is_binary: true,
                copy_from: copy_from.clone(),
                content_hashes,
            };

            let with_metadata = rebuild_metadata(data.clone(), &pointer);
            let (without, copy) = strip_metadata(&with_metadata)?;

            Ok(data == without && copy == copy_from)
        }
    }

    #[test]
    fn test_add_get_copyfrom() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir);
        let store = LfsStore::shared(&dir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::copy_from_slice(
                format!(
                    "\x01\ncopy: {}\ncopyrev: {}\n\x01\nthis is a blob",
                    k1.path, k1.hgid
                )
                .as_bytes(),
            ),
            base: None,
            key: k1.clone(),
        };

        store.add(&delta, &Default::default())?;
        let get_delta = store.get_delta(&k1)?;
        assert_eq!(Some(delta), get_delta);

        Ok(())
    }

    #[test]
    fn test_multiplexer_smaller_than_threshold() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir);
        let lfs = Arc::new(LfsStore::shared(&dir, &config)?);

        let dir = TempDir::new()?;
        let indexedlog = Arc::new(IndexedLogHgIdDataStore::new(&dir)?);

        let multiplexer = LfsMultiplexer::new(lfs, indexedlog.clone(), 10);

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };

        multiplexer.add(&delta, &Default::default())?;
        assert_eq!(multiplexer.get_delta(&k1)?, Some(delta));
        assert_eq!(indexedlog.get_missing(&[k1.into()])?, vec![]);

        Ok(())
    }

    #[test]
    fn test_multiplexer_larger_than_threshold() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir);
        let lfs = Arc::new(LfsStore::shared(&dir, &config)?);

        let dir = TempDir::new()?;
        let indexedlog = Arc::new(IndexedLogHgIdDataStore::new(&dir)?);

        let multiplexer = LfsMultiplexer::new(lfs, indexedlog.clone(), 4);

        let k1 = key("a", "3");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4, 5][..]),
            base: None,
            key: k1.clone(),
        };

        multiplexer.add(&delta, &Default::default())?;
        assert_eq!(multiplexer.get_delta(&k1)?, Some(delta));
        assert_eq!(
            indexedlog.get_missing(&[StoreKey::from(&k1)])?,
            vec![StoreKey::from(&k1)]
        );

        Ok(())
    }

    #[test]
    fn test_multiplexer_add_pointer() -> Result<()> {
        let lfsdir = TempDir::new()?;
        let config = make_lfs_config(&lfsdir);
        let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);

        let dir = TempDir::new()?;
        let indexedlog = Arc::new(IndexedLogHgIdDataStore::new(&dir)?);

        let multiplexer = LfsMultiplexer::new(lfs, indexedlog.clone(), 4);

        let sha256 =
            Sha256::from_str("4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393")?;
        let size = 12345;

        let pointer = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\nx-is-binary 0\n",
            sha256.to_hex(),
            size
        );

        let k1 = key("a", "3");
        let delta = Delta {
            data: Bytes::copy_from_slice(pointer.as_bytes()),
            base: None,
            key: k1.clone(),
        };

        multiplexer.add(
            &delta,
            &Metadata {
                size: None,
                flags: Some(Metadata::LFS_FLAG),
            },
        )?;
        assert_eq!(
            indexedlog.get_missing(&[StoreKey::from(&k1)])?,
            vec![StoreKey::from(&k1)]
        );
        // The blob isn't present, so we cannot get it.
        assert_eq!(multiplexer.get(&k1)?, None);

        multiplexer.flush()?;

        let lfs = LfsStore::shared(&lfsdir, &config)?;
        let entry = lfs.pointers.read().get(&k1)?;

        assert!(entry.is_some());

        let entry = entry.unwrap();

        assert_eq!(entry.hgid, k1.hgid);
        assert_eq!(entry.size, size);
        assert_eq!(entry.is_binary, false);
        assert_eq!(entry.copy_from, None);
        assert_eq!(
            entry.content_hashes[&ContentHashType::Sha256],
            ContentHash::Sha256(sha256)
        );

        Ok(())
    }

    #[test]
    fn test_multiplexer_add_copy_from_pointer() -> Result<()> {
        let lfsdir = TempDir::new()?;
        let config = make_lfs_config(&lfsdir);
        let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);

        let dir = TempDir::new()?;
        let indexedlog = Arc::new(IndexedLogHgIdDataStore::new(&dir)?);

        let multiplexer = LfsMultiplexer::new(lfs, indexedlog.clone(), 4);

        let sha256 =
            Sha256::from_str("4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393")?;
        let size = 12345;
        let copy_from = key("foo/bar", "1234");

        let pointer = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\nx-is-binary 1\nx-hg-copy {}\nx-hg-copyrev {}\n",
            sha256.to_hex(),
            size,
            copy_from.path,
            copy_from.hgid,
        );

        let k1 = key("a", "3");
        let delta = Delta {
            data: Bytes::copy_from_slice(pointer.as_bytes()),
            base: None,
            key: k1.clone(),
        };

        multiplexer.add(
            &delta,
            &Metadata {
                size: None,
                flags: Some(Metadata::LFS_FLAG),
            },
        )?;
        assert_eq!(
            indexedlog.get_missing(&[StoreKey::from(&k1)])?,
            vec![StoreKey::from(&k1)]
        );
        // The blob isn't present, so we cannot get it.
        assert_eq!(multiplexer.get(&k1)?, None);

        multiplexer.flush()?;

        let lfs = LfsStore::shared(&lfsdir, &config)?;
        let entry = lfs.pointers.read().get(&k1)?;

        assert!(entry.is_some());

        let entry = entry.unwrap();

        assert_eq!(entry.hgid, k1.hgid);
        assert_eq!(entry.size, size);
        assert_eq!(entry.is_binary, true);
        assert_eq!(entry.copy_from, Some(copy_from));
        assert_eq!(
            entry.content_hashes[&ContentHashType::Sha256],
            ContentHash::Sha256(sha256)
        );

        Ok(())
    }

    #[test]
    fn test_multiplexer_blob_with_header() -> Result<()> {
        let lfsdir = TempDir::new()?;
        let config = make_lfs_config(&lfsdir);
        let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);

        let dir = TempDir::new()?;
        let indexedlog = Arc::new(IndexedLogHgIdDataStore::new(&dir)?);

        let blob = Bytes::from(&b"\x01\nTHIS IS A BLOB WITH A HEADER"[..]);
        let sha256 = match ContentHash::sha256(&blob)? {
            ContentHash::Sha256(sha256) => sha256,
        };
        let size = blob.len();
        lfs.blobs.add(&sha256, blob)?;

        let multiplexer = LfsMultiplexer::new(lfs, indexedlog, 4);

        let pointer = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\nx-is-binary 0\n",
            sha256.to_hex(),
            size
        );

        let k1 = key("a", "3");
        let delta = Delta {
            data: Bytes::copy_from_slice(pointer.as_bytes()),
            base: None,
            key: k1.clone(),
        };

        multiplexer.add(
            &delta,
            &Metadata {
                size: Some(size.try_into()?),
                flags: Some(Metadata::LFS_FLAG),
            },
        )?;

        let read_blob = multiplexer.get(&k1)?.map(|vec| Bytes::from(vec));
        let expected_blob = Some(Bytes::from(
            &b"\x01\n\x01\n\x01\nTHIS IS A BLOB WITH A HEADER"[..],
        ));
        assert_eq!(read_blob, expected_blob);

        Ok(())
    }

    #[test]
    fn test_lfs_non_present() -> Result<()> {
        let cachedir = TempDir::new()?;
        let lfsdir = TempDir::new()?;
        let config = make_lfs_config(&cachedir);

        let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
        let remote = LfsRemote::new(lfs, &config)?;

        let blob = (
            Sha256::from_str("0000000000000000000000000000000000000000000000000000000000000000")?,
            1,
            Bytes::from(&b"nothing"[..]),
        );

        let resp = remote.batch(&[(blob.0, blob.1)]);
        let err = resp.err().unwrap();
        assert_eq!(err.to_string(), "Couldn't fetch oid 0000000000000000000000000000000000000000000000000000000000000000: ObjectError { code: 404, message: \"Object does not exist\" }");

        Ok(())
    }

    #[test]
    fn test_lfs_remote() -> Result<()> {
        let cachedir = TempDir::new()?;
        let lfsdir = TempDir::new()?;
        let config = make_lfs_config(&cachedir);

        let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
        let remote = LfsRemote::new(lfs, &config)?;

        let blob1 = (
            Sha256::from_str("fc613b4dfd6736a7bd268c8a0e74ed0d1c04a959f59dd74ef2874983fd443fc9")?,
            6,
            Bytes::from(&b"master"[..]),
        );
        let blob2 = (
            Sha256::from_str("ca3e228a1d8d845064112c4e92781f6b8fc2501f0aa0e415d4a1dcc941485b24")?,
            6,
            Bytes::from(&b"1.44.0"[..]),
        );

        let resp = remote
            .batch(&[(blob1.0, blob1.1), (blob2.0, blob2.1)])?
            .collect::<Result<Vec<_>>>()?
            .sort();
        assert_eq!(resp, vec![(blob1.0, blob1.2), (blob2.0, blob2.2)].sort());

        Ok(())
    }

    #[test]
    fn test_lfs_remote_file() -> Result<()> {
        let cachedir = TempDir::new()?;
        let mut config = make_lfs_config(&cachedir);

        let lfsdir = TempDir::new()?;
        let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);

        let remote = TempDir::new()?;
        let remote_lfs_file_store = LfsBlobsStore::shared(remote.path())?;

        let blob1 = (
            Sha256::from_str("fc613b4dfd6736a7bd268c8a0e74ed0d1c04a959f59dd74ef2874983fd443fc9")?,
            6,
            Bytes::from(&b"master"[..]),
        );
        let blob2 = (
            Sha256::from_str("ca3e228a1d8d845064112c4e92781f6b8fc2501f0aa0e415d4a1dcc941485b24")?,
            6,
            Bytes::from(&b"1.44.0"[..]),
        );

        remote_lfs_file_store.add(&blob1.0, blob1.2.clone())?;
        remote_lfs_file_store.add(&blob2.0, blob2.2.clone())?;

        let url = Url::from_file_path(&remote).unwrap();
        config.set("lfs", "url", Some(url.as_str()), &Default::default());

        let remote = LfsRemote::new(lfs, &config)?;

        let resp = remote
            .batch(&[(blob1.0, blob1.1), (blob2.0, blob2.1)])?
            .collect::<Result<Vec<_>>>()?
            .sort();
        assert_eq!(resp, vec![(blob1.0, blob1.2), (blob2.0, blob2.2)].sort());

        Ok(())
    }

    #[test]
    fn test_lfs_remote_datastore() -> Result<()> {
        let cachedir = TempDir::new()?;
        let lfsdir = TempDir::new()?;
        let config = make_lfs_config(&cachedir);

        let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
        let remote = LfsRemote::new(lfs.clone(), &config)?;

        let key = key("a/b", "1234");

        let mut content_hashes = HashMap::new();
        content_hashes.insert(
            ContentHashType::Sha256,
            ContentHash::Sha256(Sha256::from_str(
                "ca3e228a1d8d845064112c4e92781f6b8fc2501f0aa0e415d4a1dcc941485b24",
            )?),
        );

        let pointer = LfsPointersEntry {
            hgid: key.hgid.clone(),
            size: 6,
            is_binary: false,
            copy_from: None,
            content_hashes,
        };

        // Populate the pointer store. Usually, this would be done via a previous remotestore call.
        lfs.pointers.write().add(pointer)?;

        let remotedatastore = remote.datastore(lfs.clone());

        let expected_delta = Delta {
            data: Bytes::from(&b"1.44.0"[..]),
            base: None,
            key: key.clone(),
        };

        assert_eq!(remotedatastore.get_delta(&key)?, Some(expected_delta));

        Ok(())
    }

    #[test]
    fn test_blob() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir);
        let store = LfsStore::shared(&dir, &config)?;

        let data = Bytes::from(&[1, 2, 3, 4][..]);
        let k1 = key("a", "2");
        let delta = Delta {
            data,
            base: None,
            key: k1.clone(),
        };

        store.add(&delta, &Default::default())?;

        let blob = store.blob(&StoreKey::from(k1))?;
        assert_eq!(blob, Some(delta.data));

        Ok(())
    }

    #[test]
    fn test_metadata() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir);
        let store = LfsStore::shared(&dir, &config)?;

        let k1 = key("a", "2");
        let data = Bytes::from(&[1, 2, 3, 4][..]);
        let hash = ContentHash::sha256(&data)?;
        let delta = Delta {
            data,
            base: None,
            key: k1.clone(),
        };

        store.add(&delta, &Default::default())?;

        let metadata = store.metadata(&StoreKey::from(k1))?;
        assert_eq!(
            metadata,
            Some(ContentMetadata {
                size: 4,
                is_binary: false,
                hash,
            })
        );

        Ok(())
    }
}
