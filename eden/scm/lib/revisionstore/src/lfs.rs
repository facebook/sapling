/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    cmp::min,
    collections::{HashMap, HashSet},
    convert::TryInto,
    env::var_os,
    fs::File,
    io::{ErrorKind, Read, Write},
    iter,
    ops::Range,
    path::{Path, PathBuf},
    str::{self, FromStr},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread::sleep,
    time::Duration,
};

use anyhow::{bail, ensure, format_err, Context, Result};
use bytes::{Bytes, BytesMut};
use futures::stream::{iter, StreamExt};
use parking_lot::{Mutex, RwLock};
use rand::{thread_rng, Rng};
use reqwest::{Client, Method, Proxy, RequestBuilder, Url};
use serde_derive::{Deserialize, Serialize};
use tokio::{runtime::Runtime, task::spawn_blocking, time::timeout};
use tracing::info_span;

use configparser::{
    config::ConfigSet,
    hg::{ByteCount, ConfigSetHgExt},
};
use indexedlog::log::IndexOutput;
use lfs_protocol::{
    ObjectAction, ObjectStatus, Operation, RequestBatch, RequestObject, ResponseBatch,
    Sha256 as LfsSha256,
};
use mincode::{deserialize, serialize};
use types::{HgId, Key, RepoPath, Sha256};
use util::path::{create_dir, create_shared_dir, remove_file};

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
    util::{get_lfs_blobs_path, get_lfs_objects_path, get_lfs_pointers_path, get_str_config},
};

/// The `LfsPointersStore` holds the mapping between a `HgId` and the content hash (sha256) of the LFS blob.
struct LfsPointersStore(Store);

struct LfsIndexedLogBlobsStore {
    inner: RwLock<Store>,
    chunk_size: usize,
}

/// The `LfsBlobsStore` holds the actual blobs. Lookup is done via the content hash (sha256) of the
/// blob.
enum LfsBlobsStore {
    /// Blobs are stored on-disk and will stay on it until garbage collected.
    Loose(PathBuf, bool),

    /// Blobs are chunked and stored in an IndexedLog.
    IndexedLog(LfsIndexedLogBlobsStore),

    /// Allow blobs to be searched in both stores. Writes will only be done to the first one.
    Union(Box<LfsBlobsStore>, Box<LfsBlobsStore>),
}

struct HttpLfsRemote {
    url: Url,
    user_agent: String,
    concurrent_fetches: usize,
    backoff_times: Vec<f32>,
    request_timeout: Duration,
    client: Client,
    rt: Arc<Mutex<Runtime>>,
}

enum LfsRemoteInner {
    Http(HttpLfsRemote),
    File(LfsBlobsStore),
}

pub struct LfsRemote {
    local: Option<Arc<LfsStore>>,
    shared: Arc<LfsStore>,
    remote: LfsRemoteInner,
    move_after_upload: bool,
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
            StoreKey::Content(hash, _) => match hash {
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

#[derive(Serialize, Deserialize)]
struct LfsIndexedLogBlobsEntry {
    sha256: Sha256,
    range: Range<usize>,
    data: Bytes,
}

impl LfsIndexedLogBlobsStore {
    fn chunk_size(config: &ConfigSet) -> Result<usize> {
        Ok(config
            .get_or("lfs", "blobschunksize", || ByteCount::from(20_000_000))?
            .value() as usize)
    }

    fn open_options(config: &ConfigSet) -> Result<StoreOpenOptions> {
        let log_size =
            config.get_or("lfs", "blobsstoresize", || ByteCount::from(20_000_000_000))?;
        let auto_sync = config.get_or("lfs", "autosyncthreshold", || {
            ByteCount::from(1_000_000_000)
        })?;
        Ok(StoreOpenOptions::new()
            .max_log_count(4)
            .max_bytes_per_log(log_size.value() / 4)
            .auto_sync_threshold(auto_sync.value())
            .index("sha256", |_| {
                vec![IndexOutput::Reference(0..Sha256::len() as u64)]
            }))
    }

    pub fn shared(path: &Path, config: &ConfigSet) -> Result<Self> {
        let path = get_lfs_blobs_path(path)?;
        Ok(Self {
            inner: RwLock::new(LfsIndexedLogBlobsStore::open_options(config)?.shared(path)?),
            chunk_size: LfsIndexedLogBlobsStore::chunk_size(config)?,
        })
    }

    pub fn get(&self, hash: &Sha256) -> Result<Option<Bytes>> {
        let store = self.inner.read();
        let chunks_iter = store
            .lookup(0, hash)?
            .map(|data| Ok(deserialize::<LfsIndexedLogBlobsEntry>(data?)?));

        // Filter errors. It's possible that one entry is corrupted, or for whatever reason can't
        // be deserialized, whenever this blob/entry is refetched, the corrupted entry will still be
        // present alonside a valid one. We shouldn't fail because of it, so filter the errors.
        let mut chunks = chunks_iter
            .filter(|res| res.is_ok())
            .collect::<Result<Vec<LfsIndexedLogBlobsEntry>>>()?;
        drop(store);

        if chunks.is_empty() {
            return Ok(None);
        }

        // Make sure that the ranges are sorted in increasing order.
        chunks.sort_unstable_by(|a, b| a.range.start.cmp(&b.range.start));

        // unwrap safety: chunks isn't empty.
        let size = chunks.last().unwrap().range.end;

        let mut res = BytesMut::with_capacity(size);

        let mut next_start = 0;
        for entry in chunks.into_iter() {
            // A chunk is missing.
            if entry.range.start > next_start {
                return Ok(None);
            }

            // This chunk is fully contained in the previous ones.
            if entry.range.end <= next_start {
                continue;
            }

            let mut range_in_data = Range {
                start: 0,
                end: entry.data.len(),
            };

            // This chunk starts before the end of the previous one.
            if entry.range.start < next_start {
                range_in_data.start = next_start - entry.range.start;
            }

            next_start = entry.range.end;

            res.extend_from_slice(entry.data.slice(range_in_data).as_ref());
        }

        let data = res.freeze();
        if &ContentHash::sha256(&data).unwrap_sha256() != hash {
            Ok(None)
        } else {
            Ok(Some(data))
        }
    }

    /// Test whether a blob is in the store. It returns true if at least one chunk is present, and
    /// thus it is possible that one of the chunk is missing.
    pub fn contains(&self, hash: &Sha256) -> Result<bool> {
        Ok(self.inner.read().lookup(0, hash)?.next().is_some())
    }

    fn chunk(mut data: Bytes, chunk_size: usize) -> impl Iterator<Item = (Range<usize>, Bytes)> {
        let mut start = 0;
        iter::from_fn(move || {
            if data.len() == 0 {
                None
            } else {
                let size = min(chunk_size, data.len());
                let next = Some((start..start + size, data.split_to(size)));
                start += size;
                next
            }
        })
    }

    pub fn add(&self, hash: &Sha256, data: Bytes) -> Result<()> {
        let chunks = LfsIndexedLogBlobsStore::chunk(data, self.chunk_size);
        let chunks = chunks.map(|(range, data)| LfsIndexedLogBlobsEntry {
            sha256: hash.clone(),
            range,
            data,
        });

        for entry in chunks {
            let serialized = serialize(&entry)?;
            self.inner.write().append(serialized)?;
        }

        Ok(())
    }

    pub fn flush(&self) -> Result<()> {
        self.inner.write().flush()
    }
}

impl LfsBlobsStore {
    /// Store the local blobs in their loose format, ie: one file on disk per LFS blob.
    pub fn local(path: &Path) -> Result<Self> {
        Ok(LfsBlobsStore::Loose(get_lfs_objects_path(path)?, true))
    }

    /// Store the shared blobs in an `IndexedLog`, but still allow reading blobs in their loose
    /// format.
    pub fn shared(path: &Path, config: &ConfigSet) -> Result<Self> {
        let indexedlog = Box::new(LfsBlobsStore::IndexedLog(LfsIndexedLogBlobsStore::shared(
            &path, config,
        )?));
        let loose = Box::new(LfsBlobsStore::Loose(get_lfs_objects_path(path)?, false));

        Ok(LfsBlobsStore::union(indexedlog, loose))
    }

    /// Loose shared blob store. Intended to be used when the remote store destination is FS
    /// backed instead of HTTP backed.
    fn loose(path: PathBuf) -> Self {
        LfsBlobsStore::Loose(path, false)
    }

    fn union(first: Box<LfsBlobsStore>, second: Box<LfsBlobsStore>) -> Self {
        LfsBlobsStore::Union(first, second)
    }

    fn path(path: &Path, hash: &Sha256) -> PathBuf {
        let mut path = path.to_path_buf();
        let mut hex = hash.to_hex();

        let second = hex.split_off(2);
        path.push(hex);
        path.push(second);

        path
    }

    /// Read the blob matching the content hash.
    ///
    /// Blob hash should be validated by the underlying store.
    pub fn get(&self, hash: &Sha256) -> Result<Option<Bytes>> {
        let blob = match self {
            LfsBlobsStore::Loose(path, _) => {
                let path = LfsBlobsStore::path(&path, hash);
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
                let blob = Bytes::from(buf);
                if &ContentHash::sha256(&blob).unwrap_sha256() != hash {
                    None
                } else {
                    Some(blob)
                }
            }

            LfsBlobsStore::IndexedLog(log) => log.get(hash)?,

            LfsBlobsStore::Union(first, second) => {
                if let Some(blob) = first.get(hash)? {
                    Some(blob)
                } else {
                    second.get(hash)?
                }
            }
        };

        Ok(blob)
    }

    /// Test whether the blob store contains the hash.
    pub fn contains(&self, hash: &Sha256) -> Result<bool> {
        match self {
            LfsBlobsStore::Loose(path, _) => Ok(LfsBlobsStore::path(&path, hash).is_file()),
            LfsBlobsStore::IndexedLog(log) => log.contains(hash),
            LfsBlobsStore::Union(first, second) => {
                Ok(first.contains(hash)? || second.contains(hash)?)
            }
        }
    }

    /// Add the blob to the store.
    pub fn add(&self, hash: &Sha256, blob: Bytes) -> Result<()> {
        match self {
            LfsBlobsStore::Loose(path, is_local) => {
                let path = LfsBlobsStore::path(&path, hash);
                let parent_path = path.parent().unwrap();

                if *is_local {
                    create_dir(parent_path)?;
                } else {
                    create_shared_dir(parent_path)?;
                }

                let mut file = File::create(path)?;
                file.write_all(&blob)?;

                if *is_local {
                    file.sync_all()?;
                }
            }

            LfsBlobsStore::IndexedLog(log) => log.add(hash, blob)?,

            LfsBlobsStore::Union(first, _) => first.add(hash, blob)?,
        }

        Ok(())
    }

    pub fn remove(&self, hash: &Sha256) -> Result<()> {
        match self {
            LfsBlobsStore::Loose(path, _) => {
                let path = LfsBlobsStore::path(&path, hash);
                remove_file(path).with_context(|| format!("Cannot remove LFS blob {}", hash))?;
            }

            _ => (),
        }

        Ok(())
    }

    pub fn flush(&self) -> Result<()> {
        match self {
            LfsBlobsStore::IndexedLog(log) => log.flush(),
            LfsBlobsStore::Union(first, _) => first.flush(),
            _ => Ok(()),
        }
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
        let blobs = LfsBlobsStore::shared(path, config)?;
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
                                match self.blobs.contains(&sha256) {
                                    Ok(true) => None,
                                    Ok(false) | Err(_) => Some(StoreKey::Content(
                                        content_hash.clone(),
                                        Some(key.clone()),
                                    )),
                                }
                            }
                        },
                    }
                }
                StoreKey::Content(content_hash, key) => match content_hash {
                    ContentHash::Sha256(hash) => match self.blobs.contains(&hash) {
                        Ok(true) => None,
                        Ok(false) | Err(_) => {
                            // WARNING: Hack!
                            //
                            // For now, the only Content addressed store is the LfsStore, as such,
                            // returning a StoreKey::Content when we get here isn't going to help
                            // in finding the missing blob.
                            //
                            // If for any reason, the LFS server is turned off, we will end up in
                            // here for blobs where we have the pointer locally, but not the blob.
                            // In this case, we want the code to fallback to fetching the blob with
                            // the regular non-LFS protocol, hence we need to pretend that what is
                            // missing isn't the content hash, but the filenode hash.
                            //
                            // Obviously, the above doesn't apply for local blobs, as having these
                            // missing should be fatal.
                            let pointers = self.pointers.read();
                            if pointers.0.is_local() {
                                Some(k.clone())
                            } else {
                                match key {
                                    None => Some(StoreKey::from(content_hash)),
                                    Some(key) => Some(StoreKey::from(key)),
                                }
                            }
                        }
                    },
                },
            })
            .collect())
    }

    fn translate_lfs_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.get_missing(keys)
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
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        match self.blob_impl(&StoreKey::from(key))? {
            Some((entry, content)) => {
                let content = rebuild_metadata(content, &entry);
                // PERF: Consider changing HgIdDataStore::get() to return Bytes to avoid copying data.
                Ok(Some(content.as_ref().to_vec()))
            }
            None => Ok(None),
        }
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
        let content_hash = ContentHash::sha256(&data);

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
        self.blobs.flush()?;
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

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        self.union.get_meta(key)
    }
}

impl LocalStore for LfsMultiplexer {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.union.get_missing(keys)
    }

    fn translate_lfs_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.union.translate_lfs_missing(keys)
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
        let ret = self.non_lfs.flush()?;
        self.lfs.flush()?;
        Ok(ret)
    }
}

impl LfsRemoteInner {
    fn batch_fetch(
        &self,
        objs: &[(Sha256, usize)],
        write_to_store: impl Fn(Sha256, Bytes) -> Result<()> + Send + Clone + 'static,
    ) -> Result<()> {
        let read_from_store = |_sha256| unreachable!();
        match self {
            LfsRemoteInner::Http(http) => Self::batch_http(
                http,
                objs,
                Operation::Download,
                read_from_store,
                write_to_store,
            ),
            LfsRemoteInner::File(file) => Self::batch_fetch_file(file, objs, write_to_store),
        }
    }

    fn batch_upload(
        &self,
        objs: &[(Sha256, usize)],
        read_from_store: impl Fn(Sha256) -> Result<Option<Bytes>> + Send + Clone + 'static,
    ) -> Result<()> {
        let write_to_store = |_, _| unreachable!();
        match self {
            LfsRemoteInner::Http(http) => Self::batch_http(
                http,
                objs,
                Operation::Upload,
                read_from_store,
                write_to_store,
            ),
            LfsRemoteInner::File(file) => Self::batch_upload_file(file, objs, read_from_store),
        }
    }

    async fn send_with_retry(
        client: Client,
        method: Method,
        url: Url,
        user_agent: String,
        backoff_times: Vec<f32>,
        request_timeout: Duration,
        add_extra: impl Fn(RequestBuilder) -> RequestBuilder,
    ) -> Result<Option<Bytes>> {
        let mut backoff = backoff_times.into_iter();

        loop {
            let req = client
                .request(method.clone(), url.clone())
                .header("Accept", "application/vnd.git-lfs+json")
                .header("Content-Type", "application/vnd.git-lfs+json")
                .header("User-Agent", &user_agent);
            let req = add_extra(req);

            let reply = timeout(request_timeout, req.send())
                .await
                .with_context(|| {
                    format!("Timed out after waiting {:?} from {}", request_timeout, url)
                })??
                .error_for_status();

            let (err, status) = match reply {
                Ok(response) => return Ok(Some(response.bytes().await?)),
                Err(e) => match e.status() {
                    None => return Err(e.into()),
                    Some(status) => (e, status),
                },
            };

            if status.is_server_error() {
                if status.as_u16() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                    // No need to retry, the server is down.
                    if method == Method::GET {
                        return Ok(None);
                    } else {
                        return Err(err.into());
                    }
                }

                if let Some(backoff_time) = backoff.next() {
                    spawn_blocking(move || {
                        let mut rng = thread_rng();
                        let sleep_time = Duration::from_secs_f32(rng.gen_range(0.0, backoff_time));
                        sleep(sleep_time)
                    })
                    .await?;
                    continue;
                }
            }

            return Err(err.into());
        }
    }

    fn send_batch_request(
        http: &HttpLfsRemote,
        objs: &[(Sha256, usize)],
        operation: Operation,
    ) -> Result<Option<ResponseBatch>> {
        let span = info_span!("LfsRemote::send_batch_inner");
        let _guard = span.enter();

        let objects = objs
            .iter()
            .map(|(oid, size)| RequestObject {
                oid: LfsSha256(oid.into_inner()),
                size: *size as u64,
            })
            .collect::<Vec<_>>();

        let batch = RequestBatch {
            operation,
            transfers: vec![Default::default()],
            r#ref: None,
            objects,
        };

        let batch_json = serde_json::to_string(&batch)?;

        let response_fut = async move {
            LfsRemoteInner::send_with_retry(
                http.client.clone(),
                Method::POST,
                http.url.join("objects/batch")?,
                http.user_agent.clone(),
                http.backoff_times.clone(),
                http.request_timeout,
                move |builder| builder.body(batch_json.clone()),
            )
            .await
        };

        let response = http.rt.lock().block_on(response_fut)?;
        let response = match response {
            None => return Ok(None),
            Some(response) => response,
        };

        Ok(Some(serde_json::from_slice(response.as_ref())?))
    }

    async fn process_action(
        client: Client,
        user_agent: String,
        backoff_times: Vec<f32>,
        request_timeout: Duration,
        op: Operation,
        action: ObjectAction,
        oid: Sha256,
        read_from_store: impl Fn(Sha256) -> Result<Option<Bytes>> + Send + 'static,
        write_to_store: impl Fn(Sha256, Bytes) -> Result<()> + Send + 'static,
    ) -> Result<()> {
        let body = if op == Operation::Upload {
            spawn_blocking(move || read_from_store(oid)).await??
        } else {
            None
        };

        let method = match op {
            Operation::Download => Method::GET,
            Operation::Upload => Method::PUT,
        };

        let url = Url::from_str(&action.href.to_string())?;
        let data = LfsRemoteInner::send_with_retry(
            client,
            method,
            url,
            user_agent,
            backoff_times,
            request_timeout,
            move |mut builder| {
                if let Some(header) = action.header.as_ref() {
                    for (key, val) in header {
                        builder = builder.header(key, val)
                    }
                }

                if let Some(body) = body.clone() {
                    builder.body(body)
                } else {
                    builder.header("Content-Length", 0)
                }
            },
        )
        .await?;

        if op == Operation::Download {
            if let Some(data) = data {
                spawn_blocking(move || write_to_store(oid, data)).await??
            }
        }

        Ok(())
    }

    /// Fetch and Upload blobs from the LFS server.
    ///
    /// When uploading, the `write_to_store` is guaranteed not to be called, similarly when fetching,
    /// the `read_from_store` will not be called.
    ///
    /// The protocol is described at: https://github.com/git-lfs/git-lfs/blob/master/docs/api/batch.md
    fn batch_http(
        http: &HttpLfsRemote,
        objs: &[(Sha256, usize)],
        operation: Operation,
        read_from_store: impl Fn(Sha256) -> Result<Option<Bytes>> + Send + Clone + 'static,
        write_to_store: impl Fn(Sha256, Bytes) -> Result<()> + Send + Clone + 'static,
    ) -> Result<()> {
        let response = LfsRemoteInner::send_batch_request(http, objs, operation)?;
        let response = match response {
            None => return Ok(()),
            Some(response) => response,
        };

        let mut futures = Vec::new();

        for object in response.objects {
            let oid = object.object.oid;
            let mut actions = match object.status {
                ObjectStatus::Ok {
                    authenticated: _,
                    actions,
                } => actions,
                ObjectStatus::Err { error: e } => bail!("Couldn't fetch oid {}: {:?}", oid, e),
            };

            for (op, action) in actions.drain() {
                let client = http.client.clone();
                let user_agent = http.user_agent.clone();
                let backoff_times = http.backoff_times.clone();
                let request_timeout = http.request_timeout.clone();

                let oid = Sha256::from(oid.0);
                let read_from_store = read_from_store.clone();
                let write_to_store = write_to_store.clone();
                let fut = async move {
                    LfsRemoteInner::process_action(
                        client,
                        user_agent,
                        backoff_times,
                        request_timeout,
                        op,
                        action,
                        oid,
                        read_from_store,
                        write_to_store,
                    )
                };

                futures.push(fut);
            }
        }

        // Request a couple of blobs concurrently.
        let mut stream = iter(futures).buffer_unordered(http.concurrent_fetches);
        http.rt.lock().block_on(async {
            while let Some(next) = stream.next().await {
                next.await?
            }

            Ok(())
        })
    }

    /// Fetch files from the filesystem.
    fn batch_fetch_file(
        file: &LfsBlobsStore,
        objs: &[(Sha256, usize)],
        write_to_store: impl Fn(Sha256, Bytes) -> Result<()>,
    ) -> Result<()> {
        for (hash, _) in objs {
            if let Some(data) = file.get(hash)? {
                write_to_store(*hash, data)?;
            }
        }

        Ok(())
    }

    fn batch_upload_file(
        file: &LfsBlobsStore,
        objs: &[(Sha256, usize)],
        read_from_store: impl Fn(Sha256) -> Result<Option<Bytes>>,
    ) -> Result<()> {
        for (sha256, _) in objs {
            if let Some(blob) = read_from_store(*sha256)? {
                file.add(sha256, blob)?;
            }
        }

        Ok(())
    }
}

impl LfsRemote {
    fn make_client(config: &ConfigSet) -> Result<Client> {
        let proxy_url = if let Some(proxy_env) = var_os("https_proxy") {
            Some(proxy_env.into_string().map_err(|_| {
                format_err!("https_proxy environment variable is not a valid UTF-8 value")
            })?)
        } else if let Some(proxy_config) = config.get_opt::<String>("http_proxy", "host")? {
            Some(proxy_config)
        } else {
            None
        };

        let proxy_url = proxy_url.and_then(|s| if s.is_empty() { None } else { Some(s) });

        // The proxy can be specified without the http scheme at the beginning, for instance:
        // `http_proxy=fwdproxy:8082` is valid but isn't an http url, ie the proxy code below would
        // simply not send http traffic towards the proxy.
        //
        // To solve this, let's parse the url twice, and manually add the http scheme if needed.
        let proxy_url = if let Some(proxy_url) = proxy_url {
            let url = Url::parse(&proxy_url)?;
            if !["http", "https"].contains(&url.scheme()) {
                Some(Url::parse(&format!("http://{}", proxy_url))?)
            } else {
                Some(url)
            }
        } else {
            None
        };

        let no_proxy = config.get_or("http_proxy", "no", || "".to_string())?;
        let no_proxy = no_proxy.split(',').map(Into::into);
        let mut no_proxy = no_proxy.collect::<HashSet<String>>();

        if let Some(env_no_proxy) = var_os("no_proxy") {
            let env_no_proxy = env_no_proxy.into_string().map_err(|_| {
                format_err!("no_proxy environment variable is not a valid UTF-8 value")
            })?;
            let env_no_proxy = env_no_proxy.split(',').map(Into::into);

            no_proxy.extend(env_no_proxy);
        }

        let client = if let Some(proxy_url) = proxy_url {
            Client::builder()
                .proxy(Proxy::custom(move |url| {
                    let host = url.host_str();
                    if let Some(host) = host {
                        // The no_proxy list is expected to be fairly small, iterating over it
                        // should be OK.
                        for no_proxy_url in &no_proxy {
                            if no_proxy_url == host {
                                return None;
                            }

                            if no_proxy_url.starts_with('.') && host.ends_with(no_proxy_url) {
                                return None;
                            }
                        }

                        Some(proxy_url.clone())
                    } else {
                        None
                    }
                }))
                .build()?
        } else {
            Client::new()
        };

        Ok(client)
    }

    pub fn new(
        shared: Arc<LfsStore>,
        local: Option<Arc<LfsStore>>,
        config: &ConfigSet,
    ) -> Result<Self> {
        let mut url = get_str_config(config, "lfs", "url")?;
        // A trailing '/' needs to be present so that `Url::join` doesn't remove the reponame
        // present at the end of the config.
        url.push('/');
        let url = Url::parse(&url)?;

        let move_after_upload = config.get_or("lfs", "moveafterupload", || false)?;

        if url.scheme() == "file" {
            let path = url.to_file_path().unwrap();
            create_dir(&path)?;
            let file = LfsBlobsStore::loose(path);
            Ok(Self {
                shared,
                local,
                move_after_upload,
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

            let backoff_times = config.get_or("lfs", "backofftimes", || vec![1f32, 4f32, 8f32])?;

            let request_timeout =
                Duration::from_millis(config.get_or("lfs", "requesttimeout", || 10_000)?);

            let rt = Arc::new(Mutex::new(Runtime::new()?));
            let client = Self::make_client(config)?;

            Ok(Self {
                shared,
                local,
                move_after_upload,
                remote: LfsRemoteInner::Http(HttpLfsRemote {
                    url,
                    user_agent,
                    concurrent_fetches,
                    backoff_times,
                    request_timeout,
                    client,
                    rt,
                }),
            })
        }
    }

    fn batch_fetch(
        &self,
        objs: &[(Sha256, usize)],
        write_to_store: impl Fn(Sha256, Bytes) -> Result<()> + Send + Clone + 'static,
    ) -> Result<()> {
        self.remote.batch_fetch(objs, write_to_store)
    }

    fn batch_upload(
        &self,
        objs: &[(Sha256, usize)],
        read_from_store: impl Fn(Sha256) -> Result<Option<Bytes>> + Send + Clone + 'static,
    ) -> Result<()> {
        self.remote.batch_upload(objs, read_from_store)
    }
}

impl HgIdRemoteStore for LfsRemote {
    fn datastore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableDeltaStore>,
    ) -> Arc<dyn RemoteDataStore> {
        Arc::new(LfsRemoteStore {
            store,
            remote: self.clone(),
        })
    }

    fn historystore(
        self: Arc<Self>,
        _store: Arc<dyn HgIdMutableHistoryStore>,
    ) -> Arc<dyn RemoteHistoryStore> {
        unreachable!()
    }
}

/// Move a blob contained in `from` to the store `to`.
///
/// After this succeeds, the blob's lifetime will be similar to any shared blob, it is the caller's
/// responsability to ensure that the blob can be fetched from the LFS server.
fn move_blob(hash: &Sha256, from: &LfsStore, to: &LfsStore) -> Result<()> {
    (|| {
        let blob = from
            .blobs
            .get(hash)?
            .ok_or_else(|| format_err!("Cannot find blob for {}", hash))?;

        to.blobs.add(hash, blob)?;
        from.blobs.remove(hash)?;

        (|| -> Result<()> {
            let key = StoreKey::from(ContentHash::Sha256(*hash));
            if let Some(pointer) = from.pointers.read().entry(&key)? {
                to.pointers.write().add(pointer)?
            }
            Ok(())
        })()
        .with_context(|| format!("Cannot move pointer for {}", hash))
    })()
    .with_context(|| format!("Cannot move blob {}", hash))
}

struct LfsRemoteStore {
    store: Arc<dyn HgIdMutableDeltaStore>,
    remote: Arc<LfsRemote>,
}

impl RemoteDataStore for LfsRemoteStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        let objs = keys
            .iter()
            .map(|k| {
                if let Some(pointer) = self.remote.shared.pointers.read().entry(k)? {
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

        // If there are no objects involved at all, then don't make an (expensive) remote request!
        if objs.is_empty() {
            return Ok(());
        }

        let span = info_span!(
            "LfsRemoteStore::prefetch",
            num_blobs = objs.len(),
            size = &0
        );
        let _guard = span.enter();

        let size = Arc::new(AtomicUsize::new(0));
        self.remote.batch_fetch(&objs, {
            let remote = self.remote.clone();
            let size = size.clone();
            move |sha256, data| {
                size.fetch_add(data.len(), Ordering::Relaxed);
                remote.shared.blobs.add(&sha256, data)
            }
        })?;
        span.record("size", &size.load(Ordering::Relaxed));

        Ok(())
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let local_store = match self.remote.local.as_ref() {
            None => return Ok(keys.to_vec()),
            Some(local) => local,
        };

        let mut not_found = Vec::new();

        let objs = keys
            .iter()
            .map(|k| {
                if let Some(pointer) = local_store.pointers.read().entry(k)? {
                    match pointer.content_hashes.get(&ContentHashType::Sha256) {
                        None => Ok(None),
                        Some(content_hash) => Ok(Some((
                            content_hash.clone().unwrap_sha256(),
                            pointer.size.try_into()?,
                        ))),
                    }
                } else {
                    not_found.push(k.clone());
                    Ok(None)
                }
            })
            .filter_map(|res| res.transpose())
            .collect::<Result<Vec<_>>>()?;

        if !objs.is_empty() {
            let span = info_span!("LfsRemoteStore::upload", num_blobs = objs.len(), size = &0);
            let _guard = span.enter();

            let size = Arc::new(AtomicUsize::new(0));

            self.remote.batch_upload(&objs, {
                let local_store = local_store.clone();
                let size = size.clone();
                move |sha256| {
                    let key = StoreKey::from(ContentHash::Sha256(sha256));
                    let opt = local_store.blob(&key)?;
                    if let Some(blob) = opt.as_ref() {
                        size.fetch_add(blob.len(), Ordering::Relaxed);
                    }
                    Ok(opt)
                }
            })?;

            span.record("size", &size.load(Ordering::Relaxed));
        }

        if self.remote.move_after_upload {
            let span = info_span!("LfsRemoteStore::move_after_upload");
            let _guard = span.enter();
            // All the blobs were successfully uploaded, we can move the blobs from the local store
            // to the shared store. This is safe to do as blobs will never be collected from the
            // server once uploaded.
            for obj in objs {
                move_blob(&obj.0, local_store, &self.remote.shared)?;
            }
        }

        Ok(not_found)
    }
}

impl HgIdDataStore for LfsRemoteStore {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        let missing = self.translate_lfs_missing(&[StoreKey::hgid(key.clone())])?;
        match self.prefetch(&missing) {
            Ok(()) => self.store.get(key),
            Err(_) => Ok(None),
        }
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        let missing = self.translate_lfs_missing(&[StoreKey::hgid(key.clone())])?;
        match self.prefetch(&missing) {
            Ok(()) => self.store.get_meta(key),
            Err(_) => Ok(None),
        }
    }
}

impl LocalStore for LfsRemoteStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.store.get_missing(keys)
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
        store.flush()?;

        let indexedlog_blobs = LfsIndexedLogBlobsStore::shared(&dir.path(), &config)?;
        let hash = ContentHash::sha256(&delta.data).unwrap_sha256();

        assert!(indexedlog_blobs.contains(&hash)?);

        assert_eq!(Some(delta.data), indexedlog_blobs.get(&hash)?);

        Ok(())
    }

    #[test]
    fn test_loose() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir);
        let blob_store = LfsBlobsStore::shared(dir.path(), &config)?;
        let loose_store = LfsBlobsStore::loose(get_lfs_objects_path(dir.path())?);

        let data = Bytes::from(&[1, 2, 3, 4][..]);
        let sha256 = ContentHash::sha256(&data).unwrap_sha256();
        loose_store.add(&sha256, data.clone())?;

        assert!(blob_store.contains(&sha256)?);
        assert_eq!(blob_store.get(&sha256)?, Some(data));

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
        let stored = store.get(&k1)?;
        assert_eq!(Some(delta.data.as_ref()), stored.as_deref());

        Ok(())
    }

    #[test]
    fn test_add_get_split() -> Result<()> {
        let dir = TempDir::new()?;
        let mut config = make_lfs_config(&dir);
        config.set("lfs", "blobschunksize", Some("2"), &Default::default());

        let store = LfsStore::shared(&dir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };

        store.add(&delta, &Default::default())?;
        let stored = store.get(&k1)?;
        assert_eq!(Some(delta.data.as_ref()), stored.as_deref());

        store.flush()?;

        let stored = store.get(&k1)?;
        assert_eq!(Some(delta.data.as_ref()), stored.as_deref());

        Ok(())
    }

    #[test]
    fn test_partial_blob() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir);

        let store = LfsIndexedLogBlobsStore::shared(dir.path(), &config)?;

        let data = Bytes::from(&[1, 2, 3, 4][..]);
        let partial = data.slice(2..);
        let sha256 = ContentHash::sha256(&data).unwrap_sha256();

        let entry = LfsIndexedLogBlobsEntry {
            sha256: sha256.clone(),
            range: Range { start: 2, end: 4 },
            data: partial,
        };

        store.inner.write().append(serialize(&entry)?)?;
        store.flush()?;

        assert_eq!(store.get(&sha256)?, None);

        Ok(())
    }

    #[test]
    fn test_full_chunked() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir);

        let store = LfsIndexedLogBlobsStore::shared(dir.path(), &config)?;

        let data = Bytes::from(&[1, 2, 3, 4, 5, 6, 7][..]);
        let sha256 = ContentHash::sha256(&data).unwrap_sha256();

        let first = data.slice(0..1);
        let second = data.slice(1..4);
        let last = data.slice(4..7);

        let first_entry = LfsIndexedLogBlobsEntry {
            sha256: sha256.clone(),
            range: Range { start: 0, end: 1 },
            data: first,
        };
        store.inner.write().append(serialize(&first_entry)?)?;

        let second_entry = LfsIndexedLogBlobsEntry {
            sha256: sha256.clone(),
            range: Range { start: 1, end: 4 },
            data: second,
        };
        store.inner.write().append(serialize(&second_entry)?)?;

        let last_entry = LfsIndexedLogBlobsEntry {
            sha256: sha256.clone(),
            range: Range { start: 4, end: 7 },
            data: last,
        };
        store.inner.write().append(serialize(&last_entry)?)?;

        store.flush()?;

        assert_eq!(store.get(&sha256)?, Some(data));

        Ok(())
    }

    #[test]
    fn test_overlapped_chunked() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir);

        let store = LfsIndexedLogBlobsStore::shared(dir.path(), &config)?;

        let data = Bytes::from(&[1, 2, 3, 4, 5, 6, 7][..]);
        let sha256 = ContentHash::sha256(&data).unwrap_sha256();

        let first = data.slice(0..4);
        let second = data.slice(2..3);
        let last = data.slice(2..7);

        let first_entry = LfsIndexedLogBlobsEntry {
            sha256: sha256.clone(),
            range: Range { start: 0, end: 4 },
            data: first,
        };
        store.inner.write().append(serialize(&first_entry)?)?;

        let second_entry = LfsIndexedLogBlobsEntry {
            sha256: sha256.clone(),
            range: Range { start: 2, end: 3 },
            data: second,
        };
        store.inner.write().append(serialize(&second_entry)?)?;

        let last_entry = LfsIndexedLogBlobsEntry {
            sha256: sha256.clone(),
            range: Range { start: 2, end: 7 },
            data: last,
        };
        store.inner.write().append(serialize(&last_entry)?)?;

        store.flush()?;

        assert_eq!(store.get(&sha256)?, Some(data));

        Ok(())
    }

    quickcheck! {
        fn metadata_strip_rebuild(data: Vec<u8>, copy_from: Option<Key>) -> Result<bool> {
            let data = Bytes::from(data);

            let mut content_hashes = HashMap::new();
            content_hashes.insert(ContentHashType::Sha256, ContentHash::sha256(&data));

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
        let stored = store.get(&k1)?;
        assert_eq!(Some(delta.data.as_ref()), stored.as_deref());

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
        let stored = multiplexer.get(&k1)?;
        assert_eq!(stored.as_deref(), Some(delta.data.as_ref()));
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
        let stored = multiplexer.get(&k1)?;
        assert_eq!(stored.as_deref(), Some(delta.data.as_ref()));
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
        let sha256 = match ContentHash::sha256(&blob) {
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

    #[cfg(feature = "fb")]
    mod fb_test {
        use super::*;

        #[test]
        fn test_lfs_non_present() -> Result<()> {
            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let config = make_lfs_config(&cachedir);

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = LfsRemote::new(lfs, None, &config)?;

            let blob = (
                Sha256::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000000",
                )?,
                1,
                Bytes::from(&b"nothing"[..]),
            );

            let resp = remote.batch_fetch(&[(blob.0, blob.1)], |_, _| unreachable!());
            let err = resp.err().unwrap();
            assert_eq!(err.to_string(), "Couldn't fetch oid 0000000000000000000000000000000000000000000000000000000000000000: ObjectError { code: 404, message: \"Object does not exist\" }");

            Ok(())
        }

        #[test]
        fn test_lfs_empty_proxy() -> Result<()> {
            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let mut config = make_lfs_config(&cachedir);

            config.set("http_proxy", "host", Some(""), &Default::default());

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            LfsRemote::new(lfs, None, &config)?;

            Ok(())
        }

        #[test]
        fn test_lfs_proxy_no_http() -> Result<()> {
            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let mut config = make_lfs_config(&cachedir);

            config.set(
                "http_proxy",
                "host",
                Some("fwdproxy:8082"),
                &Default::default(),
            );

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = LfsRemote::new(lfs, None, &config)?;

            let blob = (
                Sha256::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000000",
                )?,
                1,
                Bytes::from(&b"nothing"[..]),
            );

            let resp = remote.batch_fetch(&[(blob.0, blob.1)], |_, _| unreachable!());
            assert!(resp.is_err());

            Ok(())
        }

        #[test]
        fn test_lfs_proxy_http() -> Result<()> {
            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let mut config = make_lfs_config(&cachedir);

            config.set(
                "http_proxy",
                "host",
                Some("http://fwdproxy:8082"),
                &Default::default(),
            );

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = LfsRemote::new(lfs, None, &config)?;

            let blob = (
                Sha256::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000000",
                )?,
                1,
                Bytes::from(&b"nothing"[..]),
            );

            let resp = remote.batch_fetch(&[(blob.0, blob.1)], |_, _| unreachable!());
            assert!(resp.is_err());

            Ok(())
        }

        #[test]
        fn test_lfs_no_proxy() -> Result<()> {
            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let mut config = make_lfs_config(&cachedir);

            config.set(
                "http_proxy",
                "host",
                Some("http://fwdproxy:8082"),
                &Default::default(),
            );
            config.set(
                "http_proxy",
                "no",
                Some("dewey-lfs.vip.facebook.com,mononoke-lfs.internal.tfbnw.net"),
                &Default::default(),
            );

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = LfsRemote::new(lfs, None, &config)?;

            let blob = (
                Sha256::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000000",
                )?,
                1,
                Bytes::from(&b"nothing"[..]),
            );

            let resp = remote.batch_fetch(&[(blob.0, blob.1)], |_, _| unreachable!());
            let err = resp.err().unwrap();
            assert_eq!(err.to_string(), "Couldn't fetch oid 0000000000000000000000000000000000000000000000000000000000000000: ObjectError { code: 404, message: \"Object does not exist\" }");

            Ok(())
        }

        #[test]
        fn test_lfs_no_proxy_suffix() -> Result<()> {
            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let mut config = make_lfs_config(&cachedir);

            config.set(
                "http_proxy",
                "host",
                Some("http://fwdproxy:8082"),
                &Default::default(),
            );

            config.set(
                "http_proxy",
                "no",
                Some(".facebook.com,.tfbnw.net"),
                &Default::default(),
            );

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = LfsRemote::new(lfs, None, &config)?;

            let blob = (
                Sha256::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000000",
                )?,
                1,
                Bytes::from(&b"nothing"[..]),
            );

            let resp = remote.batch_fetch(&[(blob.0, blob.1)], |_, _| unreachable!());
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
            let remote = LfsRemote::new(lfs, None, &config)?;

            let blob1 = (
                Sha256::from_str(
                    "fc613b4dfd6736a7bd268c8a0e74ed0d1c04a959f59dd74ef2874983fd443fc9",
                )?,
                6,
                Bytes::from(&b"master"[..]),
            );
            let blob2 = (
                Sha256::from_str(
                    "ca3e228a1d8d845064112c4e92781f6b8fc2501f0aa0e415d4a1dcc941485b24",
                )?,
                6,
                Bytes::from(&b"1.44.0"[..]),
            );

            let out = Arc::new(Mutex::new(Vec::new()));
            remote.batch_fetch(&[(blob1.0, blob1.1), (blob2.0, blob2.1)], {
                let out = out.clone();
                move |sha256, blob| {
                    out.lock().push((sha256, blob));
                    Ok(())
                }
            })?;
            out.lock().sort();

            let mut expected_res = vec![(blob1.0, blob1.2), (blob2.0, blob2.2)];
            expected_res.sort();

            assert_eq!(*out.lock(), expected_res);

            Ok(())
        }

        #[test]
        fn test_lfs_request_timeout() -> Result<()> {
            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let mut config = make_lfs_config(&cachedir);

            config.set("lfs", "requesttimeout", Some("0"), &Default::default());

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = LfsRemote::new(lfs, None, &config)?;

            let blob = (
                Sha256::from_str(
                    "fc613b4dfd6736a7bd268c8a0e74ed0d1c04a959f59dd74ef2874983fd443fc9",
                )?,
                6,
                Bytes::from(&b"master"[..]),
            );

            let res = remote.batch_fetch(&[(blob.0, blob.1)], |_, _| unreachable!());
            assert!(res.is_err());

            Ok(())
        }

        #[test]
        fn test_lfs_remote_datastore() -> Result<()> {
            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let config = make_lfs_config(&cachedir);

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = Arc::new(LfsRemote::new(lfs.clone(), None, &config)?);

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

            let stored = remotedatastore.get(&key)?;
            assert_eq!(stored.as_deref(), Some(expected_delta.data.as_ref()));

            Ok(())
        }
    }

    #[test]
    fn test_lfs_remote_file() -> Result<()> {
        let cachedir = TempDir::new()?;
        let mut config = make_lfs_config(&cachedir);

        let lfsdir = TempDir::new()?;
        let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);

        let remote = TempDir::new()?;
        let remote_lfs_file_store = LfsBlobsStore::Loose(remote.path().to_path_buf(), false);

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
        remote_lfs_file_store.flush()?;

        let url = Url::from_file_path(&remote).unwrap();
        config.set("lfs", "url", Some(url.as_str()), &Default::default());

        let remote = LfsRemote::new(lfs, None, &config)?;

        let out = Arc::new(Mutex::new(Vec::new()));
        remote.batch_fetch(&[(blob1.0, blob1.1), (blob2.0, blob2.1)], {
            let out = out.clone();
            move |sha256, blob| {
                out.lock().push((sha256, blob));
                Ok(())
            }
        })?;
        out.lock().sort();

        let mut expected_res = vec![(blob1.0, blob1.2), (blob2.0, blob2.2)];
        expected_res.sort();

        assert_eq!(*out.lock(), expected_res);

        Ok(())
    }

    #[test]
    fn test_lfs_upload_remote_file() -> Result<()> {
        let cachedir = TempDir::new()?;
        let mut config = make_lfs_config(&cachedir);

        let lfsdir = TempDir::new()?;
        let shared_lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
        let local_lfs = Arc::new(LfsStore::local(&lfsdir, &config)?);

        let remote_dir = TempDir::new()?;
        let remote_lfs_file_store = LfsBlobsStore::Loose(remote_dir.path().to_path_buf(), false);

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

        local_lfs.blobs.add(&blob1.0, blob1.2.clone())?;
        local_lfs.blobs.add(&blob2.0, blob2.2.clone())?;
        local_lfs.blobs.flush()?;

        let url = Url::from_file_path(&remote_dir).unwrap();
        config.set("lfs", "url", Some(url.as_str()), &Default::default());

        let remote = LfsRemote::new(shared_lfs, Some(local_lfs.clone()), &config)?;

        remote.batch_upload(&[(blob1.0, blob1.1), (blob2.0, blob2.1)], {
            move |sha256| local_lfs.blobs.get(&sha256)
        })?;

        assert_eq!(remote_lfs_file_store.get(&blob1.0)?, Some(blob1.2));
        assert_eq!(remote_lfs_file_store.get(&blob2.0)?, Some(blob2.2));

        Ok(())
    }

    #[test]
    fn test_lfs_upload_move_to_shared() -> Result<()> {
        let cachedir = TempDir::new()?;
        let mut config = make_lfs_config(&cachedir);

        let lfsdir = TempDir::new()?;
        let shared_lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
        let local_lfs = Arc::new(LfsStore::local(&lfsdir, &config)?);

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&"THIS IS A LARGE BLOB"[..]),
            base: None,
            key: k1.clone(),
        };

        local_lfs.add(&delta, &Default::default())?;

        let remote_dir = TempDir::new()?;
        let url = Url::from_file_path(&remote_dir).unwrap();
        config.set("lfs", "url", Some(url.as_str()), &Default::default());

        let remote = Arc::new(LfsRemote::new(
            shared_lfs.clone(),
            Some(local_lfs.clone()),
            &config,
        )?);
        let remote = remote.datastore(shared_lfs.clone());
        remote.upload(&[StoreKey::from(&k1)])?;

        // The blob was moved from the local store to the shared store.
        assert_eq!(local_lfs.get(&k1)?, None);
        assert_eq!(shared_lfs.get(&k1)?, Some(delta.data.to_vec()));

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
        let hash = ContentHash::sha256(&data);
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

    #[test]
    fn test_lfs_skips_server_for_empty_batch() -> Result<()> {
        let cachedir = TempDir::new()?;
        let lfsdir = TempDir::new()?;
        let mut config = make_lfs_config(&cachedir);

        let store = Arc::new(LfsStore::local(&lfsdir, &config)?);

        // 192.0.2.0 won't be routable, since that's TEST-NET-1. This test will fail if we attempt
        // to connect.
        config.set("lfs", "url", Some("http://192.0.2.0/"), &Default::default());

        let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
        let remote = Arc::new(LfsRemote::new(lfs, None, &config)?);

        let resp = remote.datastore(store).prefetch(&[]);
        assert!(resp.is_ok());

        Ok(())
    }
}
