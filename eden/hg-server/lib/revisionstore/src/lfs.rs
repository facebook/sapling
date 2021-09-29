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
    fs::File,
    io::{Cursor, ErrorKind, Read, Write},
    iter, mem,
    ops::Range,
    path::{Path, PathBuf},
    str::{self, FromStr},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
    time::Instant,
};

use anyhow::{bail, ensure, format_err, Context, Result};
use futures::{
    future::FutureExt,
    stream::{iter, StreamExt, TryStreamExt},
};
use http::status::StatusCode;
use minibytes::Bytes;
use parking_lot::{Mutex, RwLock};
use rand::{thread_rng, Rng};
use serde_derive::{Deserialize, Serialize};
use tokio::{
    task::spawn_blocking,
    time::{sleep, timeout},
};
use tracing::info_span;
use url::Url;

use async_runtime::block_on_exclusive as block_on_future;
use auth::{AuthGroup, AuthSection};
use configparser::{config::ConfigSet, convert::ByteCount};
use hg_http::http_client;
use http_client::{HttpClient, HttpClientError, HttpVersion, Method, MinTransferSpeed, Request};
use indexedlog::{log::IndexOutput, rotate, DefaultOpenOptions, Repair};
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
        HgIdMutableDeltaStore, Metadata, RemoteDataStore, StoreResult,
    },
    error::{FetchError, TransferError},
    historystore::{HgIdMutableHistoryStore, RemoteHistoryStore},
    indexedlogutil::{Store, StoreOpenOptions},
    localstore::LocalStore,
    redacted::{self, is_redacted},
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
    concurrent_fetches: usize,
    client: HttpClient,
    http_options: HttpOptions,
}

struct HttpOptions {
    accept_zstd: bool,
    http_version: HttpVersion,
    min_transfer_speed: Option<MinTransferSpeed>,
    correlator: Option<String>,
    user_agent: String,
    auth: Option<AuthGroup>,
    backoff_times: Vec<f32>,
    request_timeout: Duration,
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
    #[serde(with = "types::serde_with::hgid::tuple")]
    hgid: HgId,
    size: u64,
    is_binary: bool,
    #[serde(with = "types::serde_with::key::tuple")]
    copy_from: Option<Key>,
    /// The content_hashes will always contain at least a `ContentHashType::Sha256` entry.
    content_hashes: HashMap<ContentHashType, ContentHash>,
}

impl DefaultOpenOptions<rotate::OpenOptions> for LfsPointersStore {
    fn default_open_options() -> rotate::OpenOptions {
        Self::default_store_open_options().into_shared_open_options()
    }
}

impl LfsPointersStore {
    const INDEX_NODE: usize = 0;
    const INDEX_SHA256: usize = 1;

    fn default_store_open_options() -> StoreOpenOptions {
        StoreOpenOptions::new()
            .max_log_count(4)
            .max_bytes_per_log(40_000_000 / 4)
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
            })
    }

    fn open_options(config: &ConfigSet) -> Result<StoreOpenOptions> {
        let mut open_options = Self::default_store_open_options();
        if let Some(log_size) = config.get_opt::<ByteCount>("lfs", "pointersstoresize")? {
            open_options = open_options.max_bytes_per_log(log_size.value() / 4);
        }
        Ok(open_options)
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
    fn get(&self, key: &StoreKey) -> Result<Option<LfsPointersEntry>> {
        self.entry(key)
    }

    fn add(&mut self, entry: LfsPointersEntry) -> Result<()> {
        Ok(self.0.append(serialize(&entry)?)?)
    }
}

#[derive(Serialize, Deserialize)]
struct LfsIndexedLogBlobsEntry {
    #[serde(with = "types::serde_with::sha256::tuple")]
    sha256: Sha256,
    range: Range<usize>,
    data: Bytes,
}

impl DefaultOpenOptions<rotate::OpenOptions> for LfsIndexedLogBlobsStore {
    fn default_open_options() -> rotate::OpenOptions {
        Self::default_store_open_options().into_shared_open_options()
    }
}

impl LfsIndexedLogBlobsStore {
    fn chunk_size(config: &ConfigSet) -> Result<usize> {
        Ok(config
            .get_or("lfs", "blobschunksize", || ByteCount::from(20_000_000))?
            .value() as usize)
    }

    fn default_store_open_options() -> StoreOpenOptions {
        StoreOpenOptions::new()
            .max_log_count(4)
            .max_bytes_per_log(20_000_000_000 / 4)
            .auto_sync_threshold(1_000_000_000)
            .index("sha256", |_| {
                vec![IndexOutput::Reference(0..Sha256::len() as u64)]
            })
    }

    fn open_options(config: &ConfigSet) -> Result<StoreOpenOptions> {
        let mut open_options = Self::default_store_open_options();
        if let Some(log_size) = config.get_opt::<ByteCount>("lfs", "blobsstoresize")? {
            open_options = open_options.max_bytes_per_log(log_size.value() / 4);
        }

        if let Some(auto_sync) = config.get_opt::<ByteCount>("lfs", "autosyncthreshold")? {
            open_options = open_options.auto_sync_threshold(auto_sync.value());
        }

        Ok(open_options)
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

        let mut res = Vec::with_capacity(size);

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

        let data: Bytes = res.into();
        if &ContentHash::sha256(&data).unwrap_sha256() == hash || is_redacted(&data) {
            Ok(Some(data))
        } else {
            Ok(None)
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
                let next_data = data.slice(..size);
                data = data.slice(size..);
                let next = Some((start..start + size, next_data));
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
                if &ContentHash::sha256(&blob).unwrap_sha256() == hash || is_redacted(&blob) {
                    Some(blob)
                } else {
                    None
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

            _ => {}
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

    pub fn repair(path: impl AsRef<Path>) -> Result<String> {
        let path = path.as_ref();
        let mut repair_str = String::new();

        repair_str += &LfsPointersStore::repair(get_lfs_pointers_path(path)?)?;
        repair_str += &LfsIndexedLogBlobsStore::repair(get_lfs_blobs_path(path)?)?;

        Ok(repair_str)
    }

    fn blob_impl(&self, key: StoreKey) -> Result<StoreResult<(LfsPointersEntry, Bytes)>> {
        let pointer = self.pointers.read().entry(&key)?;

        match pointer {
            None => Ok(StoreResult::NotFound(key)),
            Some(entry) => match entry.content_hashes.get(&ContentHashType::Sha256) {
                None => Ok(StoreResult::NotFound(key)),
                Some(content_hash) => {
                    match self.blobs.get(&content_hash.clone().unwrap_sha256())? {
                        None => {
                            let hgid = match key {
                                StoreKey::HgId(hgid) => Some(hgid),
                                StoreKey::Content(_, hgid) => hgid,
                            };

                            Ok(StoreResult::NotFound(StoreKey::Content(
                                content_hash.clone(),
                                hgid,
                            )))
                        }
                        Some(blob) => Ok(StoreResult::Found((entry, blob))),
                    }
                }
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
                    let entry = self.pointers.read().get(&k.clone());
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
                StoreKey::Content(content_hash, _) => match content_hash {
                    ContentHash::Sha256(hash) => match self.blobs.contains(&hash) {
                        Ok(true) => None,
                        Ok(false) | Err(_) => Some(k.clone()),
                    },
                },
            })
            .collect())
    }
}

/// When a file was copied, Mercurial expects the blob that the store returns to contain this copy
/// information
fn rebuild_metadata(data: Bytes, entry: &LfsPointersEntry) -> Bytes {
    if let Some(copy_from) = &entry.copy_from {
        let copy_from_path: &[u8] = copy_from.path.as_ref();
        let mut ret = Vec::with_capacity(data.len() + copy_from_path.len() + 128);

        ret.extend_from_slice(&b"\x01\n"[..]);
        ret.extend_from_slice(&b"copy: "[..]);
        ret.extend_from_slice(copy_from_path);
        ret.extend_from_slice(&b"\n"[..]);
        ret.extend_from_slice(&b"copyrev: "[..]);
        ret.extend_from_slice(copy_from.hgid.to_hex().as_bytes());
        ret.extend_from_slice(&b"\n"[..]);
        ret.extend_from_slice(&b"\x01\n"[..]);
        ret.extend_from_slice(data.as_ref());
        ret.into()
    } else {
        if data.as_ref().starts_with(b"\x01\n") {
            let mut ret = Vec::with_capacity(data.len() + 4);
            ret.extend_from_slice(&b"\x01\n\x01\n"[..]);
            ret.extend_from_slice(data.as_ref());
            ret.into()
        } else {
            data
        }
    }
}

impl HgIdDataStore for LfsStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        match self.blob_impl(key)? {
            StoreResult::Found((entry, content)) => {
                let content = rebuild_metadata(content, &entry);
                // PERF: Consider changing HgIdDataStore::get() to return Bytes to avoid copying data.
                Ok(StoreResult::Found(content.as_ref().to_vec()))
            }
            StoreResult::NotFound(key) => Ok(StoreResult::NotFound(key)),
        }
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        let entry = self.pointers.read().get(&key)?;
        if let Some(entry) = entry {
            Ok(StoreResult::Found(Metadata {
                size: Some(entry.size.try_into()?),
                flags: None,
            }))
        } else {
            Ok(StoreResult::NotFound(key))
        }
    }

    fn refresh(&self) -> Result<()> {
        Ok(())
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

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
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
    fn blob(&self, key: StoreKey) -> Result<StoreResult<Bytes>> {
        match self.blob_impl(key)? {
            StoreResult::Found((_, blob)) => Ok(StoreResult::Found(blob)),
            StoreResult::NotFound(key) => Ok(StoreResult::NotFound(key)),
        }
    }

    fn metadata(&self, key: StoreKey) -> Result<StoreResult<ContentMetadata>> {
        let pointer = self.pointers.read().entry(&key)?;

        match pointer {
            None => Ok(StoreResult::NotFound(key)),
            Some(pointer_entry) => Ok(StoreResult::Found(pointer_entry.into())),
        }
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
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        self.union.get(key)
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        self.union.get_meta(key)
    }

    fn refresh(&self) -> Result<()> {
        self.union.refresh()
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

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        let ret = self.non_lfs.flush()?;
        self.lfs.flush()?;
        Ok(ret)
    }
}

impl LfsRemoteInner {
    fn batch_fetch(
        &self,
        objs: &HashSet<(Sha256, usize)>,
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
        objs: &HashSet<(Sha256, usize)>,
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
        client: &HttpClient,
        method: Method,
        url: Url,
        add_extra: impl Fn(Request) -> Request,
        http_options: &HttpOptions,
    ) -> Result<Option<Bytes>> {
        let mut backoff = http_options.backoff_times.iter().copied();
        let mut rng = thread_rng();
        let mut attempt = 0;

        loop {
            attempt += 1;

            let mut req = Request::new(url.clone(), method)
                .header("Accept", "application/vnd.git-lfs+json")
                .header("Content-Type", "application/vnd.git-lfs+json")
                .header("User-Agent", &http_options.user_agent)
                .header("X-Attempt", attempt.to_string())
                .http_version(http_options.http_version);

            if let Some(ref correlator) = http_options.correlator {
                req.set_header("X-Client-Correlator", correlator.clone());
            }

            if http_options.accept_zstd {
                req.set_header("Accept-Encoding", "zstd");
            }

            if let Some(mts) = http_options.min_transfer_speed {
                req.set_min_transfer_speed(mts);
            }

            if let Some(auth) = &http_options.auth {
                if let Some(cert) = &auth.cert {
                    req.set_cert(cert);
                }
                if let Some(key) = &auth.key {
                    req.set_key(key);
                }
                if let Some(ca) = &auth.cacerts {
                    req.set_cainfo(ca);
                }
            }

            let res = async {
                let request_timeout = http_options.request_timeout;

                let req = add_extra(req);

                let (mut stream, _) = client.send_async(vec![req])?;

                let reply = timeout(request_timeout, stream.next())
                    .await
                    .map_err(|_| TransferError::Timeout(request_timeout))?;

                let reply = match reply {
                    Some(r) => r?,
                    None => {
                        return Err(TransferError::EndOfStream);
                    }
                };

                let status = reply.status;
                let headers = reply.headers;

                if !status.is_success() {
                    return Err(TransferError::HttpStatus(status));
                }

                let start = Instant::now();
                let mut body = reply.body;
                let mut chunks: Vec<Vec<u8>> = vec![];
                while let Some(res) = timeout(request_timeout, body.next()).await.transpose() {
                    let chunk = res.map_err(|_| {
                        let request_id = headers
                            .get("x-request-id")
                            .and_then(|c| std::str::from_utf8(c.as_bytes()).ok())
                            .unwrap_or("?")
                            .into();
                        let bytes = chunks.iter().fold(0, |acc, c| acc + c.len());
                        let elapsed = start.elapsed().as_millis();
                        TransferError::ChunkTimeout {
                            timeout: request_timeout,
                            bytes,
                            elapsed,
                            request_id,
                        }
                    })??;

                    chunks.push(chunk);
                }

                let mut result = Vec::with_capacity(chunks.iter().map(|c| c.len()).sum());
                for chunk in chunks.into_iter() {
                    result.extend_from_slice(&chunk);
                }
                let result: Bytes = result.into();

                let content_encoding = headers.get("Content-Encoding");

                let result = match content_encoding
                    .map(|c| std::str::from_utf8(c.as_bytes()))
                    .transpose()
                    .with_context(|| format!("Invalid Content-Encoding: {:?}", content_encoding))
                    .map_err(TransferError::InvalidResponse)?
                {
                    Some("identity") | None => result,
                    Some("zstd") => Bytes::from(
                        zstd::stream::decode_all(Cursor::new(&result))
                            .context("Error decoding zstd stream")
                            .map_err(TransferError::InvalidResponse)?,
                    ),
                    Some(other) => {
                        return Err(TransferError::InvalidResponse(format_err!(
                            "Unsupported Content-Encoding: {}",
                            other
                        )));
                    }
                };

                Result::<_, TransferError>::Ok(Some(result))
            }
            .await;

            let error = match res {
                Ok(res) => return Ok(res),
                Err(error) => error,
            };

            let retry = match &error {
                TransferError::HttpStatus(status) => should_retry_http_status(*status),
                TransferError::HttpClientError(http_error) => should_retry_http_error(&http_error),
                TransferError::EndOfStream => false,
                TransferError::Timeout(..) => false,
                TransferError::ChunkTimeout { .. } => false,
                TransferError::InvalidResponse(..) => false,
            };

            if retry {
                if let Some(backoff_time) = backoff.next() {
                    let sleep_time = Duration::from_secs_f32(rng.gen_range(0.0..backoff_time));
                    sleep(sleep_time).await;
                    continue;
                }
            }

            return Err(FetchError { url, method, error }.into());
        }
    }

    fn send_batch_request(
        http: &HttpLfsRemote,
        objs: &HashSet<(Sha256, usize)>,
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
                &http.client,
                Method::Post,
                http.url.join("objects/batch")?,
                move |builder| builder.body(batch_json.clone()),
                &http.http_options,
            )
            .await
        };

        let response = block_on_future(response_fut)?;
        let response = match response {
            None => return Ok(None),
            Some(response) => response,
        };

        Ok(Some(serde_json::from_slice(response.as_ref())?))
    }

    async fn process_upload(
        client: &HttpClient,
        action: ObjectAction,
        oid: Sha256,
        read_from_store: impl Fn(Sha256) -> Result<Option<Bytes>> + Send + 'static,
        http_options: &HttpOptions,
    ) -> Result<()> {
        let body = spawn_blocking(move || read_from_store(oid)).await??;

        let url = Url::from_str(&action.href.to_string())?;
        LfsRemoteInner::send_with_retry(
            client,
            Method::Put,
            url,
            move |builder| {
                let builder = add_action_headers_to_request(builder, &action);

                if let Some(body) = body.as_ref() {
                    builder.body(Vec::from(body.as_ref()))
                } else {
                    builder.header("Content-Length", 0)
                }
            },
            http_options,
        )
        .await?;

        Ok(())
    }

    async fn process_download(
        client: &HttpClient,
        action: ObjectAction,
        oid: Sha256,
        write_to_store: impl Fn(Sha256, Bytes) -> Result<()> + Send + 'static,
        http_options: &HttpOptions,
    ) -> Result<()> {
        let url = Url::from_str(&action.href.to_string())?;
        let data = LfsRemoteInner::send_with_retry(
            client,
            Method::Get,
            url,
            move |builder| {
                let builder = add_action_headers_to_request(builder, &action);

                builder
            },
            http_options,
        )
        .await;

        let data = match data {
            Ok(data) => data,
            Err(err) => match err.downcast_ref::<FetchError>() {
                None => return Err(err),
                Some(fetch_error) => match fetch_error.error {
                    TransferError::HttpStatus(http::StatusCode::GONE) => {
                        Some(Bytes::from_static(redacted::REDACTED_CONTENT))
                    }
                    _ => return Err(err),
                },
            },
        };

        if let Some(data) = data {
            spawn_blocking(move || write_to_store(oid, data)).await??
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
        objs: &HashSet<(Sha256, usize)>,
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
                let oid = Sha256::from(oid.0);

                let fut = match op {
                    Operation::Upload => LfsRemoteInner::process_upload(
                        &http.client,
                        action,
                        oid,
                        read_from_store.clone(),
                        &http.http_options,
                    )
                    .left_future(),
                    Operation::Download => LfsRemoteInner::process_download(
                        &http.client,
                        action,
                        oid,
                        write_to_store.clone(),
                        &http.http_options,
                    )
                    .right_future(),
                };

                futures.push(Ok(fut));
            }
        }

        // Request a couple of blobs concurrently.
        block_on_future(iter(futures).try_for_each_concurrent(http.concurrent_fetches, |fut| fut))
    }

    /// Fetch files from the filesystem.
    fn batch_fetch_file(
        file: &LfsBlobsStore,
        objs: &HashSet<(Sha256, usize)>,
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
        objs: &HashSet<(Sha256, usize)>,
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
    pub fn new(
        shared: Arc<LfsStore>,
        local: Option<Arc<LfsStore>>,
        config: &ConfigSet,
        correlator: Option<String>,
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

            let auth = if config.get_or("lfs", "use-client-certs", || true)? {
                AuthSection::from_config(&config).best_match_for(&url)?
            } else {
                None
            };

            let user_agent = config.get_or("experimental", "lfs.user-agent", || {
                "mercurial/revisionstore".to_string()
            })?;

            let concurrent_fetches = config.get_or("lfs", "concurrentfetches", || 1)?;

            let backoff_times = config.get_or("lfs", "backofftimes", || vec![1f32, 4f32, 8f32])?;

            let request_timeout =
                Duration::from_millis(config.get_or("lfs", "requesttimeout", || 10_000)?);

            let accept_zstd = config.get_or("lfs", "accept-zstd", || true)?;

            let http_version = match config
                .get_or("lfs", "http-version", || "2".to_string())?
                .as_str()
            {
                "1.1" => HttpVersion::V11,
                "2" => HttpVersion::V2,
                x => bail!("Unsupported http_version: {}", x),
            };

            let low_speed_grace_period =
                Duration::from_millis(config.get_or("lfs", "low-speed-grace-period", || 10_000)?);
            let low_speed_min_bytes_per_second =
                config.get_opt::<u32>("lfs", "low-speed-min-bytes-per-second")?;
            let min_transfer_speed =
                low_speed_min_bytes_per_second.map(|min_bytes_per_second| MinTransferSpeed {
                    min_bytes_per_second,
                    grace_period: low_speed_grace_period,
                });

            let client = http_client("lfs");

            Ok(Self {
                shared,
                local,
                move_after_upload,
                remote: LfsRemoteInner::Http(HttpLfsRemote {
                    url,
                    concurrent_fetches,
                    client,
                    http_options: HttpOptions {
                        accept_zstd,
                        http_version,
                        min_transfer_speed,
                        correlator,
                        user_agent,
                        backoff_times,
                        request_timeout,
                        auth,
                    },
                }),
            })
        }
    }

    fn batch_fetch(
        &self,
        objs: &HashSet<(Sha256, usize)>,
        write_to_store: impl Fn(Sha256, Bytes) -> Result<()> + Send + Clone + 'static,
    ) -> Result<()> {
        self.remote.batch_fetch(objs, write_to_store)
    }

    fn batch_upload(
        &self,
        objs: &HashSet<(Sha256, usize)>,
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
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let mut not_found = Vec::new();

        let stores = if let Some(local_store) = self.remote.local.as_ref() {
            vec![self.remote.shared.clone(), local_store.clone()]
        } else {
            vec![self.remote.shared.clone()]
        };

        let mut obj_set = HashMap::new();
        let objs = keys
            .iter()
            .map(|k| {
                for store in &stores {
                    let pointers = store.pointers.read();
                    if let Some(pointer) = pointers.entry(k)? {
                        if let Some(content_hash) =
                            pointer.content_hashes.get(&ContentHashType::Sha256)
                        {
                            obj_set.insert(
                                content_hash.clone().unwrap_sha256(),
                                (k.clone(), pointers.0.is_local()),
                            );
                            return Ok(Some((
                                content_hash.clone().unwrap_sha256(),
                                pointer.size.try_into()?,
                            )));
                        }
                    }
                }

                not_found.push(k.clone());
                Ok(None)
            })
            .filter_map(|res| res.transpose())
            .collect::<Result<HashSet<_>>>()?;

        // If there are no objects involved at all, then don't make an (expensive) remote request!
        if objs.is_empty() {
            return Ok(not_found);
        }

        let span = info_span!(
            "LfsRemoteStore::prefetch",
            num_blobs = objs.len(),
            size = &0
        );
        let _guard = span.enter();

        let size = Arc::new(AtomicUsize::new(0));
        let obj_set = Arc::new(Mutex::new(obj_set));
        self.remote.batch_fetch(&objs, {
            let remote = self.remote.clone();
            let size = size.clone();
            let obj_set = obj_set.clone();

            move |sha256, data| {
                size.fetch_add(data.len(), Ordering::Relaxed);
                let (_, is_local) = obj_set
                    .lock()
                    .remove(&sha256)
                    .ok_or_else(|| format_err!("Cannot find {}", sha256))?;

                if is_local {
                    // Safe to unwrap as the sha256 is coming from a local LFS pointer.
                    remote.local.as_ref().unwrap().blobs.add(&sha256, data)
                } else {
                    remote.shared.blobs.add(&sha256, data)
                }
            }
        })?;
        span.record("size", &size.load(Ordering::Relaxed));

        let obj_set = mem::take(&mut *obj_set.lock());
        not_found.extend(obj_set.into_iter().map(|(sha256, (k, _))| match k {
            StoreKey::Content(content, hgid) => StoreKey::Content(content, hgid),
            StoreKey::HgId(hgid) => StoreKey::Content(ContentHash::Sha256(sha256), Some(hgid)),
        }));

        Ok(not_found)
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
            .collect::<Result<HashSet<_>>>()?;

        if !objs.is_empty() {
            let span = info_span!("LfsRemoteStore::upload", num_blobs = objs.len(), size = &0);
            let _guard = span.enter();

            let size = Arc::new(AtomicUsize::new(0));

            self.remote.batch_upload(&objs, {
                let local_store = local_store.clone();
                let size = size.clone();
                move |sha256| {
                    let key = StoreKey::from(ContentHash::Sha256(sha256));

                    match local_store.blob(key)? {
                        StoreResult::Found(blob) => {
                            size.fetch_add(blob.len(), Ordering::Relaxed);
                            Ok(Some(blob))
                        }
                        StoreResult::NotFound(_) => Ok(None),
                    }
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
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        match self.prefetch(&[key.clone()]) {
            Ok(_) => self.store.get(key),
            Err(_) => Ok(StoreResult::NotFound(key)),
        }
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        match self.prefetch(&[key.clone()]) {
            Ok(_) => self.store.get_meta(key),
            Err(_) => Ok(StoreResult::NotFound(key)),
        }
    }

    fn refresh(&self) -> Result<()> {
        Ok(())
    }
}

impl LocalStore for LfsRemoteStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.store.get_missing(keys)
    }
}

/// Wraps another remote store to retry fetching content-keys from their HgId keys.
///
/// If for any reason, the LFS server is turned off, we will end up in here for blobs where we have
/// the pointer locally, but not the blob.  In this case, we want the code to fallback to fetching
/// the blob with the regular non-LFS protocol, hence this stores merely translates
/// `StoreKey::Content` onto `StoreKey::HgId` before asking the non-LFS remote store to fetch data
/// for these.
pub struct LfsFallbackRemoteStore(Arc<dyn RemoteDataStore>);

impl LfsFallbackRemoteStore {
    pub fn new(wrapped_store: Arc<dyn RemoteDataStore>) -> Arc<dyn RemoteDataStore> {
        Arc::new(Self(wrapped_store))
    }
}

impl RemoteDataStore for LfsFallbackRemoteStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let mut not_found = Vec::new();
        let not_prefetched = self.0.prefetch(
            &keys
                .iter()
                .filter_map(|key| match key {
                    StoreKey::HgId(_) => {
                        not_found.push(key.clone());
                        None
                    }
                    StoreKey::Content(_, hgid) => match hgid {
                        None => {
                            not_found.push(key.clone());
                            None
                        }
                        Some(hgid) => Some(StoreKey::hgid(hgid.clone())),
                    },
                })
                .collect::<Vec<_>>(),
        )?;

        not_found.extend(not_prefetched.into_iter());
        Ok(not_found)
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }
}

impl HgIdDataStore for LfsFallbackRemoteStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        match self.prefetch(&[key.clone()]) {
            Ok(_) => self.0.get(key),
            Err(_) => Ok(StoreResult::NotFound(key)),
        }
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        match self.prefetch(&[key.clone()]) {
            Ok(_) => self.0.get_meta(key),
            Err(_) => Ok(StoreResult::NotFound(key)),
        }
    }

    fn refresh(&self) -> Result<()> {
        self.0.refresh()
    }
}

impl LocalStore for LfsFallbackRemoteStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.0.get_missing(keys)
    }
}

fn should_retry_http_status(status: StatusCode) -> bool {
    if status == StatusCode::SERVICE_UNAVAILABLE {
        return false;
    }

    if status == StatusCode::TOO_MANY_REQUESTS {
        return true;
    }

    if status.is_server_error() {
        return true;
    }

    false
}

fn should_retry_http_error(error: &HttpClientError) -> bool {
    match error {
        HttpClientError::Curl(e) => {
            e.is_couldnt_resolve_host()
                || e.is_operation_timedout()
                || e.is_send_error()
                || e.is_recv_error()
        }
        _ => false,
    }
}

fn add_action_headers_to_request(builder: Request, action: &ObjectAction) -> Request {
    let mut builder = builder;

    if let Some(header) = action.header.as_ref() {
        for (key, val) in header {
            builder = builder.header(key, val)
        }
    }

    builder
}
