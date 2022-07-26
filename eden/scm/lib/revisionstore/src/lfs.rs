/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::min;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Write;
use std::iter;
use std::mem;
use std::num::NonZeroU64;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;
use std::str;
use std::str::FromStr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::ensure;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_runtime::block_on;
use async_runtime::stream_to_iter;
use auth::AuthSection;
use configparser::config::ConfigSet;
use configparser::convert::ByteCount;
use futures::future::FutureExt;
use futures::stream::iter;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use hg_http::http_client;
use hg_http::http_config;
use http::status::StatusCode;
use http_client::Encoding;
use http_client::HttpClient;
use http_client::HttpClientError;
use http_client::HttpVersion;
use http_client::Method;
use http_client::MinTransferSpeed;
use http_client::Request;
use http_client::TlsError;
use http_client::TlsErrorKind;
use indexedlog::log::IndexOutput;
use indexedlog::rotate;
use indexedlog::DefaultOpenOptions;
use indexedlog::Repair;
use lfs_protocol::ObjectAction;
use lfs_protocol::ObjectStatus;
use lfs_protocol::Operation;
use lfs_protocol::RequestBatch;
use lfs_protocol::RequestObject;
use lfs_protocol::ResponseBatch;
use lfs_protocol::Sha256 as LfsSha256;
use mincode::deserialize;
use mincode::serialize;
use minibytes::Bytes;
use parking_lot::Mutex;
use parking_lot::RwLock;
use rand::thread_rng;
use rand::Rng;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use tokio::task::spawn_blocking;
use tokio::time::sleep;
use tokio::time::timeout;
use tracing::info_span;
use tracing::trace_span;
use tracing::Instrument;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::Sha256;
use url::Url;
use util::path::create_dir;
use util::path::create_shared_dir;
use util::path::remove_file;

use crate::datastore::strip_metadata;
use crate::datastore::ContentDataStore;
use crate::datastore::ContentMetadata;
use crate::datastore::Delta;
use crate::datastore::HgIdDataStore;
use crate::datastore::HgIdMutableDeltaStore;
use crate::datastore::Metadata;
use crate::datastore::RemoteDataStore;
use crate::datastore::StoreResult;
use crate::error::FetchError;
use crate::error::TransferError;
use crate::historystore::HgIdMutableHistoryStore;
use crate::historystore::RemoteHistoryStore;
use crate::indexedlogutil::Store;
use crate::indexedlogutil::StoreOpenOptions;
use crate::localstore::LocalStore;
use crate::redacted;
use crate::redacted::is_redacted;
use crate::remotestore::HgIdRemoteStore;
use crate::types::ContentHash;
use crate::types::StoreKey;
use crate::uniondatastore::UnionHgIdDataStore;
use crate::util::get_lfs_blobs_path;
use crate::util::get_lfs_objects_path;
use crate::util::get_lfs_pointers_path;
use crate::util::get_str_config;

/// The `LfsPointersStore` holds the mapping between a `HgId` and the content hash (sha256) of the LFS blob.
struct LfsPointersStore(Store);

pub(crate) struct LfsIndexedLogBlobsStore {
    inner: RwLock<Store>,
    chunk_size: usize,
    skip_hash_on_read: bool,
}

/// The `LfsBlobsStore` holds the actual blobs. Lookup is done via the content hash (sha256) of the
/// blob.
pub(crate) enum LfsBlobsStore {
    /// Blobs are stored on-disk and will stay on it until garbage collected.
    Loose(PathBuf, bool),

    /// Blobs are chunked and stored in an IndexedLog.
    IndexedLog(LfsIndexedLogBlobsStore),

    /// Allow blobs to be searched in both stores. Writes will only be done to the first one.
    Union(Box<LfsBlobsStore>, Box<LfsBlobsStore>),
}

pub(crate) struct HttpLfsRemote {
    url: Url,
    client: Arc<HttpClient>,
    concurrent_fetches: usize,
    download_chunk_size: Option<NonZeroU64>,
    http_options: Arc<HttpOptions>,
}

struct HttpOptions {
    accept_zstd: bool,
    http_version: HttpVersion,
    min_transfer_speed: Option<MinTransferSpeed>,
    correlator: Option<String>,
    user_agent: String,
    backoff_times: Vec<f32>,
    throttle_backoff_times: Vec<f32>,
    request_timeout: Duration,
    missing_client_certs: bool,
}

pub(crate) enum LfsRemoteInner {
    Http(HttpLfsRemote),
    File(LfsBlobsStore),
}

pub struct LfsRemote {
    local: Option<Arc<LfsStore>>,
    shared: Arc<LfsStore>,
    pub(crate) remote: LfsRemoteInner,
    move_after_upload: bool,
    ignore_prefetch_errors: bool,
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
    Debug,
    Serialize,
    Deserialize
)]
pub(crate) enum ContentHashType {
    Sha256,
}

/// On-disk format of an LFS pointer. This is directly serialized with the mincode encoding, and
/// thus changes to this structure must be done in a backward and forward compatible fashion.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct LfsPointersEntry {
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
            .auto_sync_threshold(50 * 1024 * 1024)
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
            skip_hash_on_read: config.get_or("lfs", "skiphashonread", || false)?,
        })
    }

    pub fn get(&self, hash: &Sha256, total_size: u64) -> Result<Option<Bytes>> {
        let store = self.inner.read();
        let chunks_iter = store
            .lookup(0, hash)?
            .map(|data| Ok(deserialize::<LfsIndexedLogBlobsEntry>(data?)?));

        // Filter errors. It's possible that one entry is corrupted, or for whatever reason can't
        // be deserialized, whenever this blob/entry is refetched, the corrupted entry will still be
        // present alonside a valid one. We shouldn't fail because of it, so filter the errors.
        let mut chunks: Vec<(usize, LfsIndexedLogBlobsEntry)> = chunks_iter
            .filter_map(|res: Result<_, Error>| res.ok())
            .enumerate()
            .collect();
        drop(store);

        if chunks.is_empty() {
            return Ok(None);
        }

        // Make sure that the ranges are sorted in increasing order.
        chunks.sort_unstable_by(|(a_idx, a), (b_idx, b)| {
            a.range.start.cmp(&b.range.start).then(a_idx.cmp(b_idx))
        });

        // unwrap safety: chunks isn't empty.
        let size = chunks.last().unwrap().1.range.end;

        let mut res = Vec::with_capacity(size);

        let mut next_start = 0;
        for (_, entry) in chunks.into_iter() {
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
        if self.skip_hash_on_read {
            if data.len() as u64 == total_size || is_redacted(&data) {
                Ok(Some(data))
            } else {
                Ok(None)
            }
        } else {
            let apparent_hash = ContentHash::sha256(&data).unwrap_sha256();
            if &apparent_hash == hash || is_redacted(&data) {
                Ok(Some(data))
            } else {
                if data.len() as u64 == total_size {
                    tracing::debug!(target: "lfs_read_hash_mismatch", lfs_read_hash_mismatch=&hash.to_hex()[..]);
                }
                Ok(None)
            }
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
        // Verify content integrity at write time to allow avoiding read time check.
        let apparent_hash = &ContentHash::sha256(&data).unwrap_sha256();
        if apparent_hash != hash {
            bail!("content hash mismatch: {} != {}", hash, apparent_hash);
        }

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
    pub fn get(&self, hash: &Sha256, size: u64) -> Result<Option<Bytes>> {
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

            LfsBlobsStore::IndexedLog(log) => log.get(hash, size)?,

            LfsBlobsStore::Union(first, second) => {
                if let Some(blob) = first.get(hash, size)? {
                    Some(blob)
                } else {
                    second.get(hash, size)?
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

pub(crate) enum LfsStoreEntry {
    PointerOnly(LfsPointersEntry),
    PointerAndBlob(LfsPointersEntry, Bytes),
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
                    match self
                        .blobs
                        .get(&content_hash.clone().unwrap_sha256(), entry.size)?
                    {
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

    // TODO(meyer): This is a crappy name, albeit so is blob_impl.
    /// Fetch whatever content is available for the specified StoreKey. Like blob_impl above, but returns just
    /// the LfsPointersEntry when that's all that's found. Mostly copy-pasted from blob_impl above.
    pub(crate) fn fetch_available(&self, key: &StoreKey) -> Result<Option<LfsStoreEntry>> {
        let pointer = self.pointers.read().entry(key)?;

        match pointer {
            None => Ok(None),
            Some(entry) => match entry.content_hashes.get(&ContentHashType::Sha256) {
                // TODO(meyer): The docs for LfsPointersEntry say Sha256 will always be available.
                // if it isn't, then should we bother returning the PointerOnly success, panic or return an error,
                // or return NotFound like blob_impl?
                None => Ok(Some(LfsStoreEntry::PointerOnly(entry))),
                Some(content_hash) => {
                    match self
                        .blobs
                        .get(&content_hash.clone().unwrap_sha256(), entry.size)?
                    {
                        None => Ok(Some(LfsStoreEntry::PointerOnly(entry))),
                        Some(blob) => Ok(Some(LfsStoreEntry::PointerAndBlob(entry, blob))),
                    }
                }
            },
        }
    }

    pub fn add_blob(&self, hash: &Sha256, blob: Bytes) -> Result<()> {
        self.blobs.add(hash, blob)
    }

    pub(crate) fn add_pointer(&self, pointer_entry: LfsPointersEntry) -> Result<()> {
        self.pointers.write().add(pointer_entry)
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
pub(crate) fn rebuild_metadata(data: Bytes, entry: &LfsPointersEntry) -> Bytes {
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

/// Computes an LfsPointersEntry and LFS content Blob from a Mercurial file blob.
pub(crate) fn lfs_from_hg_file_blob(
    hgid: HgId,
    raw_content: &Bytes,
) -> Result<(LfsPointersEntry, Bytes)> {
    let (data, copy_from) = strip_metadata(raw_content)?;
    let pointer = LfsPointersEntry::from_file_content(hgid, &data, copy_from)?;
    Ok((pointer, data))
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
        let (lfs_pointer_entry, blob) = lfs_from_hg_file_blob(delta.key.hgid, &delta.data)?;
        self.blobs.add(&lfs_pointer_entry.sha256(), blob)?;
        self.pointers.write().add(lfs_pointer_entry)
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
    pub(crate) fn from_bytes(data: impl AsRef<[u8]>, hgid: HgId) -> Result<Self> {
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

    /// Computes an LfsPointersEntry from a file's contents as would be written to the working copy and optional copy_from information
    pub(crate) fn from_file_content(
        hgid: HgId,
        content: &Bytes,
        copied_from: Option<Key>,
    ) -> Result<Self> {
        let content_hash = ContentHash::sha256(content);

        let mut content_hashes = HashMap::new();
        content_hashes.insert(ContentHashType::Sha256, content_hash);

        Ok(LfsPointersEntry {
            hgid,
            size: content.len().try_into()?,
            is_binary: content.as_ref().contains(&b'\0'),
            copy_from: copied_from,
            content_hashes,
        })
    }

    /// Returns the Sha256 ContentHash associated with this LfsPointersEntry
    ///
    /// Every LfsPointersEntry is guaranteed to contain at least Sha256, so this method makes it easy to access.
    pub(crate) fn sha256(&self) -> Sha256 {
        self.content_hashes[&ContentHashType::Sha256]
            .clone()
            .unwrap_sha256()
    }

    pub(crate) fn hgid(&self) -> HgId {
        self.hgid
    }

    /// Returns the size of the file referenced by this LfsPointersEntry
    pub(crate) fn size(&self) -> u64 {
        self.size
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
    pub fn batch_fetch(
        &self,
        objs: &HashSet<(Sha256, usize)>,
        write_to_store: impl FnMut(Sha256, Bytes) -> Result<()>,
        error_handler: impl FnMut(Sha256, Error),
    ) -> Result<()> {
        let read_from_store = |_sha256, _size| unreachable!();
        match self {
            LfsRemoteInner::Http(http) => Self::batch_http(
                http,
                objs,
                Operation::Download,
                read_from_store,
                write_to_store,
                error_handler,
            ),
            LfsRemoteInner::File(file) => Self::batch_fetch_file(file, objs, write_to_store),
        }
    }

    pub fn batch_upload(
        &self,
        objs: &HashSet<(Sha256, usize)>,
        read_from_store: impl Fn(Sha256, u64) -> Result<Option<Bytes>> + Send + Clone + 'static,
        error_handler: impl FnMut(Sha256, Error),
    ) -> Result<()> {
        let write_to_store = |_, _| unreachable!();
        match self {
            LfsRemoteInner::Http(http) => Self::batch_http(
                http,
                objs,
                Operation::Upload,
                read_from_store,
                write_to_store,
                error_handler,
            ),
            LfsRemoteInner::File(file) => Self::batch_upload_file(file, objs, read_from_store),
        }
    }

    async fn send_with_retry(
        client: Arc<HttpClient>,
        method: Method,
        url: Url,
        add_extra: impl Fn(Request) -> Request,
        check_status: impl Fn(StatusCode) -> Result<(), TransferError>,
        http_options: Arc<HttpOptions>,
    ) -> Result<Bytes, FetchError> {
        if http_options.missing_client_certs {
            return Err(FetchError {
                url,
                method,
                error: TransferError::HttpClientError(HttpClientError::Other(anyhow!(
                    "Certificate not found when attempting to send LFS request."
                ))),
            });
        }

        let span = trace_span!("LfsRemoteInner::send_with_retry", url = %url);

        let host_str = url.host_str().expect("No host in url").to_string();

        async move {
            let mut backoff = http_options.backoff_times.iter().copied();
            let mut throttle_backoff = http_options.throttle_backoff_times.iter().copied();
            let mut attempt = 0;

            let mut seen_error_codes = HashSet::new();

            loop {
                attempt += 1;

                let mut req = client
                    .new_request(url.clone(), method)
                    .header("Accept", "application/vnd.git-lfs+json")
                    .header("Content-Type", "application/vnd.git-lfs+json")
                    .header("User-Agent", &http_options.user_agent)
                    .header("X-Attempt", attempt.to_string())
                    .header("X-Attempts-Left", backoff.len().to_string())
                    .header("Host", host_str.clone())
                    .header(
                        "X-Throttle-Attempts-Left",
                        throttle_backoff.len().to_string(),
                    )
                    .http_version(http_options.http_version);

                if let Some(ref correlator) = http_options.correlator {
                    req.set_header("X-Client-Correlator", correlator.clone());
                }

                if http_options.accept_zstd {
                    req.set_accept_encoding([Encoding::Zstd]);
                }

                if let Some(mts) = http_options.min_transfer_speed {
                    req.set_min_transfer_speed(mts);
                }

                let res = async {
                    let request_timeout = http_options.request_timeout;

                    let req = add_extra(req);

                    let (responses, _) = client.send_async(vec![req])?;
                    let mut stream = responses.into_iter().collect::<FuturesUnordered<_>>();

                    let reply = timeout(request_timeout, stream.next())
                        .await
                        .map_err(|_| TransferError::Timeout(request_timeout))?;

                    let reply = match reply {
                        Some(r) => r?,
                        None => {
                            return Err(TransferError::EndOfStream);
                        }
                    };

                    let (head, body) = reply.into_parts();

                    let status = head.status();
                    if !status.is_success() {
                        return Err(TransferError::HttpStatus(status, head.headers().clone()));
                    }

                    check_status(status)?;

                    let start = Instant::now();
                    let mut body = body.decoded();
                    let mut chunks: Vec<Vec<u8>> = vec![];
                    while let Some(res) = timeout(request_timeout, body.next()).await.transpose() {
                        let chunk = res.map_err(|_| {
                            let request_id = head
                                .headers()
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

                    Result::<_, TransferError>::Ok(join_chunks(&chunks))
                }
                .await;

                let error = match res {
                    Ok(res) => {
                        for code in seen_error_codes {
                            // Record that we saw this error code, but it went away on retry.
                            hg_metrics::increment_counter(
                                format!("lfs.transient_error.{}.{}", method, code),
                                1,
                            );
                        }
                        hg_metrics::increment_counter(format!("lfs.success.{}", method), 1);
                        return Ok(res);
                    }
                    Err(error) => error,
                };

                let retry_strategy = match &error {
                    TransferError::HttpStatus(status, _) => {
                        seen_error_codes.insert(*status);
                        RetryStrategy::from_http_status(*status)
                    }
                    TransferError::HttpClientError(http_error) => {
                        RetryStrategy::from_http_error(&http_error)
                    }
                    TransferError::EndOfStream => RetryStrategy::NoRetry,
                    TransferError::Timeout(..) => RetryStrategy::NoRetry,
                    TransferError::ChunkTimeout { .. } => RetryStrategy::NoRetry,
                    TransferError::UnexpectedHttpStatus { .. } => RetryStrategy::NoRetry,
                    TransferError::InvalidResponse(..) => RetryStrategy::NoRetry,
                };

                let backoff_time = match retry_strategy {
                    RetryStrategy::RetryError => backoff.next(),
                    RetryStrategy::RetryThrottled => throttle_backoff.next(),
                    RetryStrategy::NoRetry => None,
                };

                if let Some(backoff_time) = backoff_time {
                    if backoff_time > 0.0 {
                        let sleep_time =
                            Duration::from_secs_f32(thread_rng().gen_range(0.0..backoff_time));
                        tracing::debug!(
                            sleep_time = ?sleep_time,
                            retry_strategy = ?retry_strategy,
                            "retry",
                        );
                        sleep(sleep_time).await;
                    }
                    continue;
                }

                if seen_error_codes.is_empty() {
                    hg_metrics::increment_counter(format!("lfs.fatal_error.{}.other", method), 1);
                }

                for code in seen_error_codes {
                    // Record that we saw this error code and ended up failing.
                    hg_metrics::increment_counter(
                        format!("lfs.fatal_error.{}.{}", method, code),
                        1,
                    );
                }

                return Err(FetchError { url, method, error });
            }
        }
        .instrument(span)
        .await
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

        let batch_url = http.url.join("objects/batch")?;

        let response_fut = async move {
            LfsRemoteInner::send_with_retry(
                http.client.clone(),
                Method::Post,
                batch_url,
                move |builder| builder.body(batch_json.clone()),
                |_| Ok(()),
                http.http_options.clone(),
            )
            .await
        };

        let response = block_on(response_fut)?;
        Ok(Some(serde_json::from_slice(response.as_ref())?))
    }

    async fn process_upload(
        client: Arc<HttpClient>,
        action: ObjectAction,
        oid: Sha256,
        size: u64,
        read_from_store: impl Fn(Sha256, u64) -> Result<Option<Bytes>> + Send + 'static,
        http_options: Arc<HttpOptions>,
    ) -> Result<()> {
        let body = spawn_blocking(move || read_from_store(oid, size)).await??;

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
            |_| Ok(()),
            http_options,
        )
        .await?;

        Ok(())
    }

    async fn process_download(
        client: Arc<HttpClient>,
        chunk_size: Option<NonZeroU64>,
        action: ObjectAction,
        oid: Sha256,
        size: u64,
        http_options: Arc<HttpOptions>,
    ) -> Result<(Sha256, Bytes)> {
        let url = Url::from_str(&action.href.to_string())?;

        let data = match chunk_size {
            Some(chunk_size) => {
                let chunk_increment = chunk_size.get() - 1;

                async {
                    let mut chunks = Vec::new();
                    let mut chunk_start = 0;

                    let file_end = size.saturating_sub(1);

                    while chunk_start < file_end {
                        let chunk_end = std::cmp::min(file_end, chunk_start + chunk_increment);
                        let range = format!("bytes={}-{}", chunk_start, chunk_end);

                        let chunk = LfsRemoteInner::send_with_retry(
                            client.clone(),
                            Method::Get,
                            url.clone(),
                            |builder| {
                                let builder = add_action_headers_to_request(builder, &action);
                                builder.header("Range", &range)
                            },
                            |status| {
                                if status == http::StatusCode::PARTIAL_CONTENT {
                                    return Ok(());
                                }

                                Err(TransferError::UnexpectedHttpStatus {
                                    expected: http::StatusCode::PARTIAL_CONTENT,
                                    received: status,
                                })
                            },
                            http_options.clone(),
                        )
                        .await?;

                        chunks.push(chunk);

                        chunk_start = chunk_end + 1;
                    }

                    Result::<_, FetchError>::Ok(join_chunks(&chunks))
                }
                .await
            }
            None => {
                LfsRemoteInner::send_with_retry(
                    client,
                    Method::Get,
                    url,
                    |builder| add_action_headers_to_request(builder, &action),
                    |_| Ok(()),
                    http_options,
                )
                .await
            }
        };

        let data = match data {
            Ok(data) => data,
            Err(err) => match err.error {
                TransferError::HttpStatus(http::StatusCode::GONE, _) => {
                    Bytes::from_static(redacted::REDACTED_CONTENT)
                }
                _ => {
                    return Err(err.into());
                }
            },
        };

        Ok((oid, data))
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
        read_from_store: impl Fn(Sha256, u64) -> Result<Option<Bytes>> + Send + Clone + 'static,
        mut write_to_store: impl FnMut(Sha256, Bytes) -> Result<()>,
        mut error_handler: impl FnMut(Sha256, Error),
    ) -> Result<()> {
        let response = LfsRemoteInner::send_batch_request(http, objs, operation)?;
        let response = match response {
            None => return Ok(()),
            Some(response) => response,
        };

        let mut futures = Vec::new();

        for object in response.objects {
            let oid = object.object.oid;
            let actions = match object.status {
                ObjectStatus::Ok {
                    authenticated: _,
                    actions,
                } => Some(actions),
                ObjectStatus::Err { error: e } => {
                    error_handler(
                        Sha256::from(oid.0),
                        anyhow!("LFS fetch error {} - {}", e.code, e.message),
                    );
                    None
                }
            };

            for (op, action) in actions.into_iter().map(|h| h.into_iter()).flatten() {
                let oid = Sha256::from(oid.0);

                let fut = match op {
                    Operation::Upload => LfsRemoteInner::process_upload(
                        http.client.clone(),
                        action,
                        oid,
                        object.object.size,
                        read_from_store.clone(),
                        http.http_options.clone(),
                    )
                    .map(|_| None)
                    .left_future(),
                    Operation::Download => LfsRemoteInner::process_download(
                        http.client.clone(),
                        http.download_chunk_size,
                        action,
                        oid,
                        object.object.size,
                        http.http_options.clone(),
                    )
                    .map(Some)
                    .right_future(),
                };

                futures.push(fut);
            }
        }

        // Request blobs concurrently.
        let stream = stream_to_iter(iter(futures).buffer_unordered(http.concurrent_fetches));

        // It's awkward that the futures are shared for uploading and downloading. We use Some(_)
        // to indicate if the result came from the download path, and 'flatten' filters out the
        // Nones.
        for result in stream.flatten() {
            let (sha, data) = result?;
            write_to_store(sha, data)?;
        }

        Ok(())
    }

    /// Fetch files from the filesystem.
    fn batch_fetch_file(
        file: &LfsBlobsStore,
        objs: &HashSet<(Sha256, usize)>,
        mut write_to_store: impl FnMut(Sha256, Bytes) -> Result<()>,
    ) -> Result<()> {
        for (hash, size) in objs {
            if let Some(data) = file.get(hash, *size as u64)? {
                write_to_store(*hash, data)?;
            }
        }

        Ok(())
    }

    fn batch_upload_file(
        file: &LfsBlobsStore,
        objs: &HashSet<(Sha256, usize)>,
        read_from_store: impl Fn(Sha256, u64) -> Result<Option<Bytes>>,
    ) -> Result<()> {
        for (sha256, size) in objs {
            if let Some(blob) = read_from_store(*sha256, *size as u64)? {
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
        let ignore_prefetch_errors = config.get_or("lfs", "ignore-prefetch-errors", || false)?;

        if url.scheme() == "file" {
            let path = url.to_file_path().unwrap();
            create_dir(&path)?;
            let file = LfsBlobsStore::loose(path);
            Ok(Self {
                shared,
                local,
                ignore_prefetch_errors,
                move_after_upload,
                remote: LfsRemoteInner::File(file),
            })
        } else {
            if !["http", "https"].contains(&url.scheme()) {
                bail!("Unsupported url: {}", url);
            }

            let use_client_certs = config.get_or("lfs", "use-client-certs", || true)?;

            let auth = if use_client_certs {
                AuthSection::from_config(config).best_match_for(&url)?
            } else {
                None
            };

            let missing_client_certs = use_client_certs && auth.is_none();

            let user_agent = config.get_or("experimental", "lfs.user-agent", || {
                format!("EdenSCM/{}", ::version::VERSION)
            })?;

            let concurrent_fetches = config.get_or("lfs", "concurrentfetches", || 4)?;

            let backoff_times = config.get_or("lfs", "backofftimes", || vec![1f32, 4f32, 8f32])?;

            // Backoff throtling is a lot more aggressive. This is here to mitigate large surges in
            // downloads when new LFS content is checked in. There's no way to eliminate those
            // without seriously overprovisioning. Retrying for a longer period of time is simply a
            // way to wait until whatever surge of traffic is happening ends.
            let throttle_backoff_times = config.get_or("lfs", "throttlebackofftimes", || {
                vec![
                    1f32, 4f32, 8f32, 8f32, 8f32, 8f32, 8f32, 8f32, 8f32, 8f32, 8f32, 8f32,
                ]
            })?;

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

            let download_chunk_size = config.get_opt::<u64>("lfs", "download-chunk-size")?;
            let download_chunk_size = download_chunk_size
                .map(|s| NonZeroU64::new(s).context("download chunk size cannot be 0"))
                .transpose()?;

            let client = http_client("lfs", http_config(config, auth));

            Ok(Self {
                shared,
                local,
                move_after_upload,
                ignore_prefetch_errors,
                remote: LfsRemoteInner::Http(HttpLfsRemote {
                    url,
                    client: Arc::new(client),
                    concurrent_fetches,
                    download_chunk_size,
                    http_options: Arc::new(HttpOptions {
                        accept_zstd,
                        http_version,
                        min_transfer_speed,
                        correlator,
                        user_agent,
                        backoff_times,
                        throttle_backoff_times,
                        request_timeout,
                        missing_client_certs,
                    }),
                }),
            })
        }
    }

    fn batch_fetch(
        &self,
        objs: &HashSet<(Sha256, usize)>,
        write_to_store: impl FnMut(Sha256, Bytes) -> Result<()>,
        error_handler: impl FnMut(Sha256, Error),
    ) -> Result<()> {
        self.remote.batch_fetch(objs, write_to_store, error_handler)
    }

    fn batch_upload(
        &self,
        objs: &HashSet<(Sha256, usize)>,
        read_from_store: impl Fn(Sha256, u64) -> Result<Option<Bytes>> + Send + Clone + 'static,
        error_handler: impl FnMut(Sha256, Error),
    ) -> Result<()> {
        self.remote
            .batch_upload(objs, read_from_store, error_handler)
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
fn move_blob(hash: &Sha256, size: u64, from: &LfsStore, to: &LfsStore) -> Result<()> {
    (|| {
        let blob = from
            .blobs
            .get(hash, size)?
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
        self.remote.batch_fetch(
            &objs,
            {
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
            },
            |_, _| {},
        )?;
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

            self.remote.batch_upload(
                &objs,
                {
                    let local_store = local_store.clone();
                    let size = size.clone();
                    move |sha256, _size| {
                        let key = StoreKey::from(ContentHash::Sha256(sha256));

                        match local_store.blob(key)? {
                            StoreResult::Found(blob) => {
                                size.fetch_add(blob.len(), Ordering::Relaxed);
                                Ok(Some(blob))
                            }
                            StoreResult::NotFound(_) => Ok(None),
                        }
                    }
                },
                |_, _| {},
            )?;

            span.record("size", &size.load(Ordering::Relaxed));
        }

        if self.remote.move_after_upload {
            let span = info_span!("LfsRemoteStore::move_after_upload");
            let _guard = span.enter();
            // All the blobs were successfully uploaded, we can move the blobs from the local store
            // to the shared store. This is safe to do as blobs will never be collected from the
            // server once uploaded.
            for obj in objs {
                move_blob(&obj.0, obj.1 as u64, local_store, &self.remote.shared)?;
            }
        }

        Ok(not_found)
    }
}

impl HgIdDataStore for LfsRemoteStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        match self.prefetch(&[key.clone()]) {
            Ok(_) => self.store.get(key),
            Err(_) if self.remote.ignore_prefetch_errors => Ok(StoreResult::NotFound(key)),
            Err(e) => Err(e.context(format!("Failed to fetch: {:?}", key))),
        }
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        match self.prefetch(&[key.clone()]) {
            Ok(_) => self.store.get_meta(key),
            Err(_) if self.remote.ignore_prefetch_errors => Ok(StoreResult::NotFound(key)),
            Err(e) => Err(e.context(format!("Failed to fetch: {:?}", key))),
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

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum RetryStrategy {
    RetryError,
    RetryThrottled,
    NoRetry,
}

impl RetryStrategy {
    pub fn from_http_status(status: StatusCode) -> Self {
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Self::RetryThrottled;
        }

        if status == StatusCode::REQUEST_TIMEOUT {
            return Self::RetryError;
        }

        if status.is_server_error() {
            return Self::RetryError;
        }

        Self::NoRetry
    }

    pub fn from_http_error(error: &HttpClientError) -> Self {
        use HttpClientError::*;
        let retry = match error {
            Tls(TlsError { kind, .. }) => kind == &TlsErrorKind::RecvError,
            _ => true,
        };

        if retry {
            Self::RetryError
        } else {
            Self::NoRetry
        }
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

fn join_chunks<T: AsRef<[u8]>>(chunks: &[T]) -> Bytes {
    let mut result = Vec::with_capacity(chunks.iter().map(|c| c.as_ref().len()).sum());
    for chunk in chunks {
        result.extend_from_slice(chunk.as_ref());
    }
    result.into()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use quickcheck::quickcheck;
    use tempfile::TempDir;
    use types::testutil::*;

    use super::*;
    use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
    use crate::indexedlogutil::StoreType;
    use crate::localstore::ExtStoredPolicy;
    use crate::testutil::example_blob;
    use crate::testutil::example_blob2;
    use crate::testutil::get_lfs_batch_mock;
    use crate::testutil::get_lfs_download_mock;
    use crate::testutil::make_lfs_config;
    use crate::testutil::nonexistent_blob;
    use crate::testutil::TestBlob;

    #[test]
    fn test_new_shared() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir, "test_new_shared");
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
        let config = make_lfs_config(&dir, "test_new_local");
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
        let config = make_lfs_config(&dir, "test_add");
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

        assert_eq!(
            Some(delta.data.clone()),
            indexedlog_blobs.get(&hash, delta.data.len() as u64)?
        );

        Ok(())
    }

    #[test]
    fn test_loose() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir, "test_loose");
        let blob_store = LfsBlobsStore::shared(dir.path(), &config)?;
        let loose_store = LfsBlobsStore::loose(get_lfs_objects_path(dir.path())?);

        let data = Bytes::from(&[1, 2, 3, 4][..]);
        let sha256 = ContentHash::sha256(&data).unwrap_sha256();
        loose_store.add(&sha256, data.clone())?;

        assert!(blob_store.contains(&sha256)?);
        assert_eq!(blob_store.get(&sha256, data.len() as u64)?, Some(data));

        Ok(())
    }

    #[test]
    fn test_add_get_missing() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir, "test_add_get_missing");
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
        let config = make_lfs_config(&dir, "test_add_get");
        let store = LfsStore::shared(&dir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };

        store.add(&delta, &Default::default())?;
        let stored = store.get(StoreKey::hgid(k1))?;
        assert_eq!(StoreResult::Found(delta.data.as_ref().to_vec()), stored);

        Ok(())
    }

    #[test]
    fn test_invalid_hash() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir, "test_invalid_hash");

        let store = LfsIndexedLogBlobsStore::shared(dir.path(), &config)?;

        let bad_hash = ContentHash::sha256(&Bytes::from_static(b"wrong")).unwrap_sha256();
        let data = Bytes::from_static(b"oops");

        assert!(store.add(&bad_hash, data.clone()).is_err());
        store.flush()?;

        assert_eq!(store.get(&bad_hash, data.len() as u64)?, None);

        Ok(())
    }

    #[test]
    fn test_prefer_newer_chunks() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir, "test_invalid_hash");

        let store = LfsIndexedLogBlobsStore::shared(dir.path(), &config)?;

        let data = Bytes::from_static(b"data");
        let hash = ContentHash::sha256(&data).unwrap_sha256();

        // Insert some poisoned chunks under the same hash.
        store
            .inner
            .write()
            .append(serialize(&LfsIndexedLogBlobsEntry {
                sha256: hash.clone(),
                range: (0..2),
                data: Bytes::from_static(b"oo"),
            })?)?;
        store
            .inner
            .write()
            .append(serialize(&LfsIndexedLogBlobsEntry {
                sha256: hash.clone(),
                range: (2..4),
                data: Bytes::from_static(b"ps"),
            })?)?;

        // Insert the new, correct data.
        store.add(&hash, data.clone())?;

        store.flush()?;

        // Make sure we get the new data.
        assert_eq!(store.get(&hash, data.len() as u64)?, Some(data));

        Ok(())
    }

    #[test]
    fn test_add_get_split() -> Result<()> {
        let dir = TempDir::new()?;
        let mut config = make_lfs_config(&dir, "test_add_get_split");
        config.set("lfs", "blobschunksize", Some("2"), &Default::default());

        let store = LfsStore::shared(&dir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };

        store.add(&delta, &Default::default())?;
        let k = StoreKey::hgid(k1);
        let stored = store.get(k.clone())?;
        assert_eq!(StoreResult::Found(delta.data.as_ref().to_vec()), stored);

        store.flush()?;

        let stored = store.get(k)?;
        assert_eq!(StoreResult::Found(delta.data.as_ref().to_vec()), stored);

        Ok(())
    }

    #[test]
    fn test_partial_blob() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir, "test_partial_blob");

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

        assert_eq!(store.get(&sha256, data.len() as u64)?, None);

        Ok(())
    }

    #[test]
    fn test_full_chunked() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir, "test_full_chunked");

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

        assert_eq!(store.get(&sha256, data.len() as u64)?, Some(data));

        Ok(())
    }

    #[test]
    fn test_overlapped_chunked() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir, "test_overlapped_chunked");

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

        assert_eq!(store.get(&sha256, data.len() as u64)?, Some(data));

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
        let config = make_lfs_config(&dir, "test_add_get_copyform");
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
        let stored = store.get(StoreKey::hgid(k1))?;
        assert_eq!(StoreResult::Found(delta.data.as_ref().to_vec()), stored);

        Ok(())
    }

    #[test]
    fn test_multiplexer_smaller_than_threshold() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir, "test_multiplexer_smaller_than_threshold");
        let lfs = Arc::new(LfsStore::shared(&dir, &config)?);

        let dir = TempDir::new()?;
        let indexedlog = Arc::new(IndexedLogHgIdDataStore::new(
            &dir,
            ExtStoredPolicy::Ignore,
            &ConfigSet::new(),
            StoreType::Shared,
        )?);

        let multiplexer = LfsMultiplexer::new(lfs, indexedlog.clone(), 10);

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };

        multiplexer.add(&delta, &Default::default())?;
        let k = StoreKey::hgid(k1);
        let stored = multiplexer.get(k.clone())?;
        assert_eq!(stored, StoreResult::Found(delta.data.as_ref().to_vec()));
        assert_eq!(indexedlog.get_missing(&[k])?, vec![]);

        Ok(())
    }

    #[test]
    fn test_multiplexer_larger_than_threshold() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir, "test_multiplexer_larger_than_threshold");
        let lfs = Arc::new(LfsStore::shared(&dir, &config)?);

        let dir = TempDir::new()?;
        let indexedlog = Arc::new(IndexedLogHgIdDataStore::new(
            &dir,
            ExtStoredPolicy::Ignore,
            &ConfigSet::new(),
            StoreType::Shared,
        )?);

        let multiplexer = LfsMultiplexer::new(lfs, indexedlog.clone(), 4);

        let k1 = key("a", "3");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4, 5][..]),
            base: None,
            key: k1.clone(),
        };

        multiplexer.add(&delta, &Default::default())?;
        let k = StoreKey::hgid(k1);
        let stored = multiplexer.get(k.clone())?;
        assert_eq!(stored, StoreResult::Found(delta.data.as_ref().to_vec()));
        assert_eq!(
            indexedlog.get_missing(&[k.clone()])?,
            vec![StoreKey::from(k)]
        );

        Ok(())
    }

    #[test]
    fn test_multiplexer_add_pointer() -> Result<()> {
        let lfsdir = TempDir::new()?;
        let config = make_lfs_config(&lfsdir, "test_multiplexer_add_pointer");
        let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);

        let dir = TempDir::new()?;
        let indexedlog = Arc::new(IndexedLogHgIdDataStore::new(
            &dir,
            ExtStoredPolicy::Ignore,
            &ConfigSet::new(),
            StoreType::Shared,
        )?);

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
        let k = StoreKey::hgid(k1.clone());
        assert_eq!(indexedlog.get_missing(&[k.clone()])?, vec![k.clone()]);
        // The blob isn't present, so we cannot get it.
        assert_eq!(
            multiplexer.get(k.clone())?,
            StoreResult::NotFound(StoreKey::Content(
                ContentHash::Sha256(sha256.clone()),
                Some(k1.clone())
            ))
        );

        multiplexer.flush()?;

        let lfs = LfsStore::shared(&lfsdir, &config)?;
        let entry = lfs.pointers.read().get(&k)?;

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
        let config = make_lfs_config(&lfsdir, "test_multiplexer_add_copy_pointer");
        let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);

        let dir = TempDir::new()?;
        let indexedlog = Arc::new(IndexedLogHgIdDataStore::new(
            &dir,
            ExtStoredPolicy::Ignore,
            &ConfigSet::new(),
            StoreType::Shared,
        )?);

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
        let k = StoreKey::hgid(k1.clone());
        assert_eq!(indexedlog.get_missing(&[k.clone()])?, vec![k.clone()]);
        // The blob isn't present, so we cannot get it.
        assert_eq!(
            multiplexer.get(k.clone())?,
            StoreResult::NotFound(StoreKey::Content(
                ContentHash::Sha256(sha256.clone()),
                Some(k1.clone())
            ))
        );

        multiplexer.flush()?;

        let lfs = LfsStore::shared(&lfsdir, &config)?;
        let entry = lfs.pointers.read().get(&k.clone())?;

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
        let config = make_lfs_config(&lfsdir, "test_multiplexer_blob_with_header");
        let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);

        let dir = TempDir::new()?;
        let indexedlog = Arc::new(IndexedLogHgIdDataStore::new(
            &dir,
            ExtStoredPolicy::Ignore,
            &ConfigSet::new(),
            StoreType::Shared,
        )?);

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

        let read_blob = multiplexer.get(StoreKey::hgid(k1))?;
        let expected_blob =
            StoreResult::Found(b"\x01\n\x01\n\x01\nTHIS IS A BLOB WITH A HEADER".to_vec());
        assert_eq!(read_blob, expected_blob);

        Ok(())
    }

    #[cfg(feature = "fb")]
    mod fb_test {
        use std::env::set_var;
        use std::sync::atomic::AtomicBool;

        #[cfg(fbcode_build)]
        use mockito::mock;
        use parking_lot::Mutex;

        use super::*;

        #[derive(Clone)]
        struct Sentinel(Arc<AtomicBool>);

        impl Sentinel {
            fn new() -> Self {
                Self(Arc::new(AtomicBool::new(false)))
            }

            fn set(&self) {
                self.0.store(true, Ordering::Relaxed);
            }

            fn get(&self) -> bool {
                self.0.load(Ordering::Relaxed)
            }

            fn as_callback(&self) -> impl Fn(Sha256, Bytes) -> Result<()> + Send + Clone + 'static {
                let this = self.clone();
                move |_, _| {
                    this.set();
                    Ok(())
                }
            }
        }

        #[test]
        fn test_lfs_proxy_non_present() -> Result<()> {
            let _env_lock = crate::env_lock();

            let sentinel = Sentinel::new();
            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let config = make_lfs_config(&cachedir, "test_lfs_proxy_non_present");

            let blob = &example_blob();
            let _m1 = get_lfs_batch_mock(200, &[blob]);

            let _m2 = get_lfs_download_mock(200, blob);

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = LfsRemote::new(lfs, None, &config, None)?;

            let objs = [(blob.sha, blob.size)]
                .iter()
                .cloned()
                .collect::<HashSet<_>>();
            remote.batch_fetch(&objs, sentinel.as_callback(), |_, _| {})?;
            assert!(sentinel.get());

            Ok(())
        }

        #[test]
        #[cfg(not(windows))]
        fn test_lfs_proxy_no_http() -> Result<()> {
            let _env_lock = crate::env_lock();

            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let config = make_lfs_config(&cachedir, "test_lfs_proxy_no_http");

            set_var("https_proxy", "fwdproxy:8082");

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = LfsRemote::new(lfs, None, &config, None)?;

            let blob = example_blob();
            let objs = [(blob.sha, blob.size)]
                .iter()
                .cloned()
                .collect::<HashSet<_>>();
            let resp = remote.batch_fetch(&objs, |_, _| unreachable!(), |_, _| {});
            // ex. [56] Failure when receiving data from the peer (Proxy CONNECT aborted)
            // But not necessarily that message in all cases.
            assert!(resp.is_err());

            Ok(())
        }

        #[test]
        #[cfg(not(windows))]
        fn test_lfs_proxy_http() -> Result<()> {
            let _env_lock = crate::env_lock();

            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let config = make_lfs_config(&cachedir, "test_lfs_proxy_http");

            set_var("https_proxy", "http://fwdproxy:8082");

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = LfsRemote::new(lfs, None, &config, None)?;

            let blob = example_blob();
            let objs = [(blob.sha, blob.size)]
                .iter()
                .cloned()
                .collect::<HashSet<_>>();
            let resp = remote.batch_fetch(&objs, |_, _| unreachable!(), |_, _| {});
            assert!(resp.is_err());

            Ok(())
        }

        #[test]
        fn test_lfs_no_proxy() -> Result<()> {
            let _env_lock = crate::env_lock();

            let sentinel = Sentinel::new();
            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let config = make_lfs_config(&cachedir, "test_lfs_no_proxy");

            let blob = &example_blob();
            let _m1 = get_lfs_batch_mock(200, &[blob]);

            let _m2 = get_lfs_download_mock(200, blob);

            set_var("http_proxy", "http://shouldnt-touch-this:8082");
            set_var("NO_PROXY", "localhost,127.0.0.1");

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = LfsRemote::new(lfs, None, &config, None)?;

            let objs = [(blob.sha, blob.size)]
                .iter()
                .cloned()
                .collect::<HashSet<_>>();
            remote.batch_fetch(&objs, sentinel.as_callback(), |_, _| {})?;
            assert!(sentinel.get());

            Ok(())
        }

        fn test_download<C>(configure: C, blobs: &[&TestBlob]) -> Result<()>
        where
            C: for<'a> FnOnce(&'a mut ConfigSet),
        {
            let _env_lock = crate::env_lock();

            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let mut config = make_lfs_config(&cachedir, "test_download");
            configure(&mut config);
            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = LfsRemote::new(lfs, None, &config, None)?;

            let _mocks: Vec<_> = blobs
                .iter()
                .map(|b| get_lfs_download_mock(200, b))
                .collect();

            let objs = [
                (blobs[0].sha, blobs[0].size),
                (blobs[1].sha, blobs[1].size),
                (blobs[2].sha, blobs[2].size),
            ]
            .iter()
            .cloned()
            .collect::<HashSet<_>>();

            let out = Arc::new(Mutex::new(Vec::new()));
            remote.batch_fetch(
                &objs,
                {
                    let out = out.clone();
                    move |sha256, blob| {
                        out.lock().push((sha256, blob));
                        Ok(())
                    }
                },
                |_, _| {},
            )?;
            out.lock().sort();

            let mut expected_res = vec![
                (blobs[0].sha, blobs[0].content.clone()),
                (blobs[1].sha, blobs[1].content.clone()),
            ];
            expected_res.sort();

            assert_eq!(*out.lock(), expected_res);

            Ok(())
        }

        #[test]
        fn test_lfs_remote_http1_1() -> Result<()> {
            let b1 = example_blob();
            let b2 = example_blob2();
            let b3 = nonexistent_blob();

            let blobs = vec![&b1, &b2, &b3];

            let _m1 = get_lfs_batch_mock(200, &blobs);

            test_download(
                |config| {
                    config.set("lfs", "http-version", Some("1.1"), &Default::default());
                },
                &blobs,
            )
        }

        #[test]
        fn test_lfs_remote_http2() -> Result<()> {
            let b1 = example_blob();
            let b2 = example_blob2();
            let b3 = nonexistent_blob();

            let blobs = vec![&b1, &b2, &b3];

            let _m1 = get_lfs_batch_mock(200, &blobs);

            test_download(
                |config| {
                    config.set("lfs", "http-version", Some("2"), &Default::default());
                },
                &blobs,
            )
        }

        #[test]
        fn test_lfs_remote_chunked() -> Result<()> {
            let mut blob1 = example_blob();
            let mut blob2 = example_blob2();
            blob1.chunk_size = Some(3);
            blob2.chunk_size = Some(3);
            blob1.response = vec![b"mas", b"ter"];
            blob2.response = vec![b"1.4", b"4.0"];
            let b3 = nonexistent_blob();
            let blobs = vec![&blob1, &blob2, &b3];

            let _m1 = get_lfs_batch_mock(200, &blobs);

            test_download(
                |config| {
                    config.set("lfs", "download-chunk-size", Some("3"), &Default::default());
                },
                &blobs,
            )
        }

        #[test]
        fn test_lfs_invalid_http() -> Result<()> {
            let _env_lock = crate::env_lock();

            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let mut config = make_lfs_config(&cachedir, "test_lfs_invalid_http");
            config.set("lfs", "http-version", Some("3"), &Default::default());

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config).unwrap());
            let result = LfsRemote::new(lfs, None, &config, None);

            assert!(result.is_err());

            Ok(())
        }

        #[test]
        fn test_lfs_request_timeout() -> Result<()> {
            let _env_lock = crate::env_lock();

            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let mut config = make_lfs_config(&cachedir, "test_lfs_request_timeout");

            config.set("lfs", "requesttimeout", Some("0"), &Default::default());

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = LfsRemote::new(lfs, None, &config, None)?;

            let blob = (
                Sha256::from_str(
                    "fc613b4dfd6736a7bd268c8a0e74ed0d1c04a959f59dd74ef2874983fd443fc9",
                )?,
                6,
                Bytes::from(&b"master"[..]),
            );

            let objs = [(blob.0, blob.1)].iter().cloned().collect::<HashSet<_>>();
            let res = remote.batch_fetch(&objs, |_, _| unreachable!(), |_, _| {});
            assert!(res.is_err());

            Ok(())
        }

        #[test]
        fn test_lfs_remote_datastore() -> Result<()> {
            let _env_lock = crate::env_lock();

            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let config = make_lfs_config(&cachedir, "test_lfs_remote_datastore");

            let blob = &example_blob();

            let _m1 = get_lfs_batch_mock(200, &[blob]);
            let _m2 = get_lfs_download_mock(200, blob);

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = Arc::new(LfsRemote::new(lfs.clone(), None, &config, None)?);

            let key = key("a/b", "1234");

            let mut content_hashes = HashMap::new();
            content_hashes.insert(ContentHashType::Sha256, ContentHash::Sha256(blob.sha));

            let pointer = LfsPointersEntry {
                hgid: key.hgid.clone(),
                size: blob.size as u64,
                is_binary: false,
                copy_from: None,
                content_hashes,
            };

            // Populate the pointer store. Usually, this would be done via a previous remotestore call.
            lfs.pointers.write().add(pointer)?;

            let remotedatastore = remote.datastore(lfs.clone());

            let expected_delta = Delta {
                data: blob.content.clone(),
                base: None,
                key: key.clone(),
            };

            let stored = remotedatastore.get(StoreKey::hgid(key))?;
            assert_eq!(
                stored,
                StoreResult::Found(expected_delta.data.as_ref().to_vec())
            );

            Ok(())
        }

        #[cfg(fbcode_build)]
        #[test]
        fn test_lfs_redacted() -> Result<()> {
            let _env_lock = crate::env_lock();

            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let mut config = make_lfs_config(&cachedir, "test_lfs_redacted");
            config.set(
                "lfs",
                "url",
                Some(&[mockito::server_url(), "/repo".to_string()].join("")),
                &Default::default(),
            );

            let blob = &example_blob();

            let _m1 = get_lfs_batch_mock(200, &[blob]);

            let _m2 = mock("GET", format!("/repo/download/{}", blob.oid).as_str())
                .with_status(410)
                .create();

            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = LfsRemote::new(lfs, None, &config, None)?;

            let objs = [(blob.sha, blob.size)]
                .iter()
                .cloned()
                .collect::<HashSet<_>>();
            remote.batch_fetch(
                &objs,
                |_, data| {
                    assert!(is_redacted(&data));
                    Ok(())
                },
                |_, _| {},
            )?;

            Ok(())
        }
    }

    #[test]
    fn test_lfs_remote_file() -> Result<()> {
        let _env_lock = crate::env_lock();

        let cachedir = TempDir::new()?;
        let mut config = make_lfs_config(&cachedir, "test_lfs_remote_file");

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

        let remote = LfsRemote::new(lfs, None, &config, None)?;

        let objs = [(blob1.0, blob1.1), (blob2.0, blob2.1)]
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let out = Arc::new(Mutex::new(Vec::new()));
        remote.batch_fetch(
            &objs,
            {
                let out = out.clone();
                move |sha256, blob| {
                    out.lock().push((sha256, blob));
                    Ok(())
                }
            },
            |_, _| {},
        )?;
        out.lock().sort();

        let mut expected_res = vec![(blob1.0, blob1.2), (blob2.0, blob2.2)];
        expected_res.sort();

        assert_eq!(*out.lock(), expected_res);

        Ok(())
    }

    #[test]
    fn test_lfs_upload_remote_file() -> Result<()> {
        let _env_lock = crate::env_lock();

        let cachedir = TempDir::new()?;
        let mut config = make_lfs_config(&cachedir, "test_lfs_upload_remote_file");

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

        let remote = LfsRemote::new(shared_lfs, Some(local_lfs.clone()), &config, None)?;

        let objs = [(blob1.0, blob1.1), (blob2.0, blob2.1)]
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        remote.batch_upload(
            &objs,
            move |sha256, size| local_lfs.blobs.get(&sha256, size),
            |_, _| {},
        )?;

        assert_eq!(
            remote_lfs_file_store.get(&blob1.0, blob1.1 as u64)?,
            Some(blob1.2)
        );
        assert_eq!(
            remote_lfs_file_store.get(&blob2.0, blob2.1 as u64)?,
            Some(blob2.2)
        );

        Ok(())
    }

    #[test]
    fn test_lfs_upload_move_to_shared() -> Result<()> {
        let _env_lock = crate::env_lock();

        let cachedir = TempDir::new()?;
        let mut config = make_lfs_config(&cachedir, "test_lfs_upload_move_to_shared");

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
            None,
        )?);
        let remote = remote.datastore(shared_lfs.clone());
        let k = StoreKey::hgid(k1.clone());
        remote.upload(&[k.clone()])?;

        let contentk = StoreKey::Content(ContentHash::sha256(&delta.data), Some(k1));

        // The blob was moved from the local store to the shared store.
        assert_eq!(local_lfs.get(k.clone())?, StoreResult::NotFound(contentk));
        assert_eq!(
            shared_lfs.get(k.clone())?,
            StoreResult::Found(delta.data.to_vec())
        );

        Ok(())
    }

    #[test]
    fn test_blob() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir, "test_blob");
        let store = LfsStore::shared(&dir, &config)?;

        let data = Bytes::from(&[1, 2, 3, 4][..]);
        let k1 = key("a", "2");
        let delta = Delta {
            data,
            base: None,
            key: k1.clone(),
        };

        store.add(&delta, &Default::default())?;

        let blob = store.blob(StoreKey::from(k1))?;
        assert_eq!(blob, StoreResult::Found(delta.data));

        Ok(())
    }

    #[test]
    fn test_metadata() -> Result<()> {
        let dir = TempDir::new()?;
        let config = make_lfs_config(&dir, "test_metadata");
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

        let metadata = store.metadata(StoreKey::from(k1))?;
        assert_eq!(
            metadata,
            StoreResult::Found(ContentMetadata {
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
        let mut config = make_lfs_config(&cachedir, "test_lfs_skips_server_for_empty_batch");

        let store = Arc::new(LfsStore::local(&lfsdir, &config)?);

        // 192.0.2.0 won't be routable, since that's TEST-NET-1. This test will fail if we attempt
        // to connect.
        config.set("lfs", "url", Some("http://192.0.2.0/"), &Default::default());

        let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
        let remote = Arc::new(LfsRemote::new(lfs, None, &config, None)?);

        let resp = remote.datastore(store).prefetch(&[]);
        assert!(resp.is_ok());

        Ok(())
    }

    #[test]
    fn test_should_retry() {
        assert_eq!(
            RetryStrategy::from_http_status(StatusCode::CONTINUE),
            RetryStrategy::NoRetry
        );
        assert_eq!(
            RetryStrategy::from_http_status(StatusCode::OK),
            RetryStrategy::NoRetry
        );
        assert_eq!(
            RetryStrategy::from_http_status(StatusCode::MOVED_PERMANENTLY),
            RetryStrategy::NoRetry
        );
        assert_eq!(
            RetryStrategy::from_http_status(StatusCode::BAD_REQUEST),
            RetryStrategy::NoRetry
        );
        assert_eq!(
            RetryStrategy::from_http_status(StatusCode::NOT_ACCEPTABLE),
            RetryStrategy::NoRetry
        );
        assert_eq!(
            RetryStrategy::from_http_status(StatusCode::SERVICE_UNAVAILABLE),
            RetryStrategy::RetryError
        );

        assert_eq!(
            RetryStrategy::from_http_status(StatusCode::INTERNAL_SERVER_ERROR),
            RetryStrategy::RetryError
        );
        assert_eq!(
            RetryStrategy::from_http_status(StatusCode::BAD_GATEWAY),
            RetryStrategy::RetryError
        );

        assert_eq!(
            RetryStrategy::from_http_status(StatusCode::TOO_MANY_REQUESTS),
            RetryStrategy::RetryThrottled
        );
    }

    #[test]
    fn test_lfs_zero_or_empty_backoff() -> Result<()> {
        let test_with_config = |backoff_config: &'static str| -> Result<()> {
            let blob1 = example_blob();
            let blobs = vec![&blob1];
            let req_count = if backoff_config.is_empty() {
                1
            } else {
                backoff_config.split(',').count() + 1
            };

            let m1 = get_lfs_batch_mock(200, &blobs).expect(1);
            let m2 = get_lfs_download_mock(408, &blob1)
                .pop()
                .unwrap()
                .expect(req_count);
            let cachedir = TempDir::new()?;
            let mut config = make_lfs_config(&cachedir, "test_download");

            config.set(
                "lfs",
                "backofftimes",
                Some(backoff_config),
                &Default::default(),
            );

            let lfsdir = TempDir::new()?;
            let lfs = Arc::new(LfsStore::shared(&lfsdir, &config)?);
            let remote = LfsRemote::new(lfs, None, &config, None)?;
            let objs = [(blobs[0].sha, blobs[0].size)]
                .iter()
                .cloned()
                .collect::<HashSet<_>>();

            // Make sure we get an error (but don't panic).
            assert!(remote.batch_fetch(&objs, |_, _| Ok(()), |_, _| {}).is_err());

            // Check request count.
            m1.assert();
            m2.assert();

            Ok(())
        };

        test_with_config("")?;
        test_with_config("0")?;
        test_with_config("0,0,0")?;

        Ok(())
    }
}
