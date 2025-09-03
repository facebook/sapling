/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp::min;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Write;
use std::iter;
use std::num::NonZeroU64;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;
use std::str;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::format_err;
use async_runtime::block_on;
use async_runtime::stream_to_iter;
use blob::Blob;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::convert::ByteCount;
use format_util::strip_file_metadata;
use fs_err::File;
use futures::future::FutureExt;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use futures::stream::iter;
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
use http_client::curl;
use indexedlog::DefaultOpenOptions;
use indexedlog::Repair;
use indexedlog::log::IndexOutput;
use indexedlog::rotate;
use indexedlog::rotate::ConsistentReadGuard;
use lfs_protocol::ObjectAction;
use lfs_protocol::ObjectStatus;
use lfs_protocol::Operation;
use lfs_protocol::RequestBatch;
use lfs_protocol::RequestObject;
use lfs_protocol::ResponseBatch;
use lfs_protocol::Sha256 as LfsSha256;
use mincode::deserialize;
use mincode::serialize;
use mincode::serialize_into;
use minibytes::Bytes;
use rand::Rng;
use rand::thread_rng;
use redacted::is_redacted;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use sha2::Digest;
use storemodel::SerializationFormat;
use tokio::task::spawn_blocking;
use tokio::time::sleep;
use tokio::time::timeout;
use tracing::Instrument;
use tracing::info_span;
use tracing::trace_span;
use tracing::warn;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::Sha256;
use url::Url;
use util::path::create_dir;
use util::path::create_shared_dir;
use util::path::remove_file;

use crate::datastore::ContentMetadata;
use crate::datastore::StoreResult;
use crate::error::FetchError;
use crate::error::TransferError;
use crate::indexedlogutil::Store;
use crate::indexedlogutil::StoreOpenOptions;
use crate::types::ContentHash;
use crate::types::StoreKey;
use crate::util::get_lfs_blobs_path;
use crate::util::get_lfs_objects_path;
use crate::util::get_lfs_pointers_path;

/// The `LfsPointersStore` holds the mapping between a `HgId` and the content hash (sha256) of the LFS blob.
struct LfsPointersStore(Store);

#[derive(Clone)]
pub struct LfsIndexedLogBlobsStore {
    inner: Arc<Store>,
    chunk_size: usize,
}

/// The `LfsBlobsStore` holds the actual blobs. Lookup is done via the content hash (sha256) of the
/// blob.
#[derive(Clone)]
pub enum LfsBlobsStore {
    /// Blobs are stored on-disk and will stay on it until garbage collected.
    Loose(PathBuf, bool),

    /// Blobs are chunked and stored in an IndexedLog.
    IndexedLog(LfsIndexedLogBlobsStore),

    /// Allow blobs to be searched in both stores. Writes will only be done to the first one.
    Union(Arc<LfsBlobsStore>, Arc<LfsBlobsStore>),
}

pub struct HttpLfsRemote {
    url: Url,
    client: Arc<HttpClient>,
    concurrent_fetches: usize,
    download_chunk_size: NonZeroU64,
    http_options: Arc<HttpOptions>,
    buf_pool: LimitedBufferPool,
}

struct HttpOptions {
    accept_zstd: bool,
    http_version: HttpVersion,
    min_transfer_speed: Option<MinTransferSpeed>,
    user_agent: String,
    backoff_times: Vec<f32>,
    throttle_backoff_times: Vec<f32>,
    request_timeout: Duration,
}

pub enum LfsRemote {
    Http(HttpLfsRemote),
    File(LfsBlobsStore),
}

#[derive(Clone)]
pub struct LfsClient {
    pub(crate) local: Option<Arc<LfsStore>>,
    pub(crate) shared: Arc<LfsStore>,
    pub(crate) remote: Arc<LfsRemote>,
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
    pointers: LfsPointersStore,
    blobs: LfsBlobsStore,
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
        Self::default_store_open_options(&BTreeMap::<&str, &str>::new()).into_rotated_open_options()
    }
}

impl LfsPointersStore {
    const INDEX_NODE: usize = 0;
    const INDEX_SHA256: usize = 1;

    fn default_store_open_options(config: &dyn Config) -> StoreOpenOptions {
        StoreOpenOptions::new(config)
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

    fn open_options(config: &dyn Config) -> Result<StoreOpenOptions> {
        let mut open_options = Self::default_store_open_options(config);
        if let Some(log_size) = config.get_opt::<ByteCount>("lfs", "pointersstoresize")? {
            open_options = open_options.max_bytes_per_log(log_size.value() / 4);
        }
        Ok(open_options)
    }

    /// Create a permanent `LfsPointersStore`.
    fn permanent(path: &Path, config: &dyn Config) -> Result<Self> {
        let path = get_lfs_pointers_path(path)?;
        Ok(Self(
            LfsPointersStore::open_options(config)?.permanent(path)?,
        ))
    }

    /// Create a rotated `LfsPointersStore`.
    fn rotated(path: &Path, config: &dyn Config) -> Result<Self> {
        let path = get_lfs_pointers_path(path)?;
        Ok(Self(LfsPointersStore::open_options(config)?.rotated(path)?))
    }

    /// Read an entry from the slice and deserialize it.
    fn get_from_slice(data: &[u8]) -> Result<LfsPointersEntry> {
        Ok(deserialize(data)?)
    }

    /// Find the pointer corresponding to the passed in `StoreKey`.
    fn entry(&self, key: &StoreKey) -> Result<Option<LfsPointersEntry>> {
        let log = self.0.read();
        let mut iter = match key {
            StoreKey::HgId(key) => log.lookup(Self::INDEX_NODE, key.hgid)?,
            StoreKey::Content(hash, _) => match hash {
                ContentHash::Sha256(hash) => log.lookup(Self::INDEX_SHA256, hash)?,
            },
        };

        let buf = match iter.next() {
            None => return Ok(None),
            Some(buf) => buf?,
        };

        Self::get_from_slice(buf).map(Some)
    }

    /// Find the pointer corresponding to the passed in `HgId`.
    fn get_by_hgid(&self, hgid: &HgId) -> Result<Option<LfsPointersEntry>> {
        let log = self.0.read();
        let mut iter = log.lookup(Self::INDEX_NODE, hgid)?;
        let buf = match iter.next() {
            None => return Ok(None),
            Some(buf) => buf?,
        };
        Self::get_from_slice(buf).map(Some)
    }

    fn add(&self, entry: LfsPointersEntry) -> Result<()> {
        self.0.append(serialize(&entry)?)
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
        Self::default_store_open_options(&BTreeMap::<&str, &str>::new()).into_rotated_open_options()
    }
}

impl LfsIndexedLogBlobsStore {
    fn chunk_size(config: &dyn Config) -> Result<usize> {
        Ok(config
            .get_or("lfs", "blobschunksize", || ByteCount::from(20_000_000))?
            .value() as usize)
    }

    fn default_store_open_options(config: &dyn Config) -> StoreOpenOptions {
        StoreOpenOptions::new(config)
            .max_log_count(4)
            .max_bytes_per_log(20_000_000_000 / 4)
            .auto_sync_threshold(250 * 1024 * 1024)
            .load_specific_config(config, "lfs")
            .index("sha256", |_| {
                vec![IndexOutput::Reference(0..Sha256::len() as u64)]
            })
    }

    fn open_options(config: &dyn Config) -> Result<StoreOpenOptions> {
        let mut open_options = Self::default_store_open_options(config);
        if let Some(log_size) = config.get_opt::<ByteCount>("lfs", "blobsstoresize")? {
            open_options = open_options.max_bytes_per_log(log_size.value() / 4);
        }

        if let Some(auto_sync) = config.get_opt::<ByteCount>("lfs", "autosyncthreshold")? {
            open_options = open_options.auto_sync_threshold(auto_sync.value());
        }

        Ok(open_options)
    }

    pub fn rotated(path: &Path, config: &dyn Config) -> Result<Self> {
        let path = get_lfs_blobs_path(path)?;
        Ok(Self {
            inner: Arc::new(LfsIndexedLogBlobsStore::open_options(config)?.rotated(path)?),
            chunk_size: LfsIndexedLogBlobsStore::chunk_size(config)?,
        })
    }

    pub fn get(&self, hash: &Sha256, total_size: u64) -> Result<Option<Blob>> {
        let log = self.inner.read();
        let chunks_iter = log.lookup(0, hash)?.map(|data| {
            let data: Bytes = log.slice_to_bytes(data?);
            let deserialized: LfsIndexedLogBlobsEntry =
                data.as_deserialize_hint(|| deserialize(&data))?;
            Ok(deserialized)
        });

        // Filter errors. It's possible that one entry is corrupted, or for whatever reason can't
        // be deserialized, whenever this blob/entry is refetched, the corrupted entry will still be
        // present alonside a valid one. We shouldn't fail because of it, so filter the errors.
        let mut chunks: Vec<(usize, LfsIndexedLogBlobsEntry)> = chunks_iter
            .filter_map(|res: Result<_, Error>| res.ok())
            .enumerate()
            .collect();

        if chunks.is_empty() {
            return Ok(None);
        }

        // Make sure that the ranges are sorted in increasing order.
        chunks.sort_unstable_by(|(a_idx, a), (b_idx, b)| {
            a.range.start.cmp(&b.range.start).then(a_idx.cmp(b_idx))
        });

        // unwrap safety: chunks isn't empty.
        let size = chunks.last().unwrap().1.range.end;

        let mut blob_builder = blob::Builder::with_capacity(size);

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

            blob_builder.append(entry.data.slice(range_in_data));
        }

        let data = blob_builder.into_blob();

        // Skip SHA256 hash check on reading. Data integrity is checked by indexedlog xxhash and
        // length. The SHA256 check can be slow (~90% of data reading time!).
        if data.len() as u64 == total_size || is_redacted(&data) {
            Ok(Some(data))
        } else {
            Ok(None)
        }
    }

    /// Test whether a blob is in the store. It returns true if at least one chunk is present, and
    /// thus it is possible that one of the chunk is missing.
    pub fn contains(&self, hash: &Sha256) -> Result<bool> {
        Ok(!self.inner.read().lookup(0, hash)?.is_empty()?)
    }

    fn chunk(mut data: Bytes, chunk_size: usize) -> impl Iterator<Item = (Range<usize>, Bytes)> {
        let mut start = 0;
        iter::from_fn(move || {
            if data.is_empty() {
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
            self.inner.append(serialized)?;
        }

        Ok(())
    }

    pub fn flush(&self) -> Result<()> {
        self.inner.flush()
    }
}

impl LfsBlobsStore {
    /// Store the blobs in their loose format, ie: one file on disk per LFS blob.
    pub fn loose_objects(path: &Path) -> Result<Self> {
        Ok(LfsBlobsStore::Loose(get_lfs_objects_path(path)?, true))
    }

    /// Store the blobs in a rotated `IndexedLog`, but still allow reading blobs in their loose
    /// format.
    pub fn rotated_or_loose_objects(path: &Path, config: &dyn Config) -> Result<Self> {
        let indexedlog = Arc::new(LfsBlobsStore::IndexedLog(LfsIndexedLogBlobsStore::rotated(
            path, config,
        )?));
        let loose = Arc::new(LfsBlobsStore::Loose(get_lfs_objects_path(path)?, false));

        Ok(LfsBlobsStore::union(indexedlog, loose))
    }

    /// Loose shared blob store. Intended to be used when the remote store destination is FS
    /// backed instead of HTTP backed.
    fn loose(path: PathBuf) -> Self {
        LfsBlobsStore::Loose(path, false)
    }

    fn union(first: Arc<LfsBlobsStore>, second: Arc<LfsBlobsStore>) -> Self {
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
    pub fn get(&self, hash: &Sha256, size: u64) -> Result<Option<Blob>> {
        let blob = match self {
            LfsBlobsStore::Loose(path, _) => {
                let path = LfsBlobsStore::path(path, hash);
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
                let bytes = Bytes::from(buf);
                let apparent_hash = ContentHash::sha256(&bytes).unwrap_sha256();
                let blob = Blob::from(bytes);
                if &apparent_hash == hash || is_redacted(&blob) {
                    Some(blob)
                } else {
                    None
                }
            }

            LfsBlobsStore::IndexedLog(log) => log.get(hash, size)?,

            LfsBlobsStore::Union(first, second) => match first.get(hash, size)? {
                Some(blob) => Some(blob),
                _ => second.get(hash, size)?,
            },
        };

        Ok(blob)
    }

    /// Test whether the blob store contains the hash.
    pub fn contains(&self, hash: &Sha256) -> Result<bool> {
        match self {
            LfsBlobsStore::Loose(path, _) => Ok(LfsBlobsStore::path(path, hash).is_file()),
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
                let mut file = create_loose_file(path, hash, *is_local)?;
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
                let path = LfsBlobsStore::path(path, hash);
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

fn create_loose_file(path: &Path, hash: &Sha256, is_local: bool) -> Result<File> {
    let path = LfsBlobsStore::path(path, hash);
    let parent_path = path.parent().unwrap();

    if is_local {
        create_dir(parent_path)?;
    } else {
        create_shared_dir(parent_path)?;
    }

    Ok(File::create(path)?)
}

/// Streaming LFS chunk inserter to insert into LFS blob store without holding the entire blob in
/// memory.
struct StreamingInserter {
    expected_hash: Sha256,
    got_hash: sha2::Sha256,
    size: u64,
    written_so_far: u64,
    redacted: bool,
    state: StreamingState,
}

enum StreamingState {
    // Store, reusable buffer, written so far
    Log(LfsIndexedLogBlobsStore, Vec<u8>, usize),
    // File to append chunks to
    File(File),
    // Store in memory
    Memory(Vec<u8>),
}

impl StreamingState {
    fn write_chunk(&mut self, chunk: Bytes, hash: Sha256, force: bool) -> Result<Option<Vec<u8>>> {
        match self {
            StreamingState::Log(store, buf, len) => {
                let mut took_chunk = false;

                match chunk.take_vec() {
                    Ok(chunk_vec) => {
                        // If buf is empty and chunk_vec is bigger, just use it. The main goal here
                        // is to optimize allocations when a single network chunk satisfies
                        // store.chunk_size (i.e. we don't need to allocate buf at all).
                        if buf.is_empty() && buf.capacity() < chunk_vec.len() {
                            *buf = chunk_vec;
                            took_chunk = true;
                        } else {
                            buf.extend_from_slice(&chunk_vec);
                        }
                    }
                    Err(chunk) => {
                        buf.extend_from_slice(chunk.as_ref());
                    }
                }

                // Accumulate chunks until we get to the desired storage chunk size.
                if force || buf.len() >= store.chunk_size {
                    let data_len = buf.len();
                    let entry = LfsIndexedLogBlobsEntry {
                        sha256: hash,
                        range: *len..*len + data_len,
                        // Convert our reusable buf into Bytes, temporarily.
                        data: std::mem::take(buf).into(),
                    };
                    store
                        .inner
                        .append_direct(|buf| Ok(serialize_into(buf, &entry)?))?;
                    *len += data_len;

                    if took_chunk {
                        // If we took the vec out of `chunk`, return it back to caller so it can be reused.
                        return Ok(entry.data.take_vec().ok());
                    } else {
                        // Now that we are done serializing, recover our buf for future buffering.
                        *buf = entry.data.into_vec();
                        buf.clear();
                    }
                }
                Ok(None)
            }
            StreamingState::File(file) => {
                file.write_all(&chunk)?;
                Ok(chunk.take_vec().ok())
            }
            StreamingState::Memory(buf) => {
                buf.extend_from_slice(chunk.as_ref());
                Ok(None)
            }
        }
    }
}

impl StreamingInserter {
    pub(crate) fn new(store: &LfsBlobsStore, hash: Sha256, size: u64) -> Result<Self> {
        match &store {
            LfsBlobsStore::Loose(path, local) => Ok(Self {
                expected_hash: hash,
                got_hash: sha2::Sha256::new(),
                size,
                written_so_far: 0,
                redacted: false,
                state: StreamingState::File(create_loose_file(path, &hash, *local)?),
            }),
            LfsBlobsStore::IndexedLog(store) => Ok(Self {
                expected_hash: hash,
                got_hash: sha2::Sha256::new(),
                size,
                written_so_far: 0,
                redacted: false,
                state: StreamingState::Log(store.clone(), Vec::new(), 0),
            }),
            LfsBlobsStore::Union(a, _) => Self::new(a, hash, size),
        }
    }

    pub(crate) fn memory(hash: Sha256, size: u64) -> Self {
        Self {
            expected_hash: hash,
            got_hash: sha2::Sha256::new(),
            size,
            written_so_far: 0,
            redacted: false,
            state: StreamingState::Memory(Vec::with_capacity(size as usize)),
        }
    }

    pub(crate) fn add_chunk(&mut self, chunk: Bytes) -> Result<Option<Vec<u8>>> {
        if self.redacted {
            bail!("can't add chunk - already redacted");
        }

        if self.written_so_far + chunk.len() as u64 > self.size {
            bail!(
                "too much data written: {} > {}",
                self.written_so_far,
                self.size
            );
        }

        self.got_hash.update(chunk.as_ref());

        if self.written_so_far + chunk.len() as u64 == self.size {
            // Check content hash before inserting the final chunk.
            let got_hash: [u8; Sha256::len()] =
                std::mem::take(&mut self.got_hash).finalize().into();
            let got_hash = Sha256::from(got_hash);
            if got_hash != self.expected_hash {
                bail!(
                    "content hash mismatch: {} != {}",
                    self.expected_hash,
                    got_hash
                );
            }
        }

        self.written_so_far += chunk.len() as u64;

        if let Some(mut buf) =
            self.state
                .write_chunk(chunk, self.expected_hash, self.written_so_far == self.size)?
        {
            buf.clear();
            return Ok(Some(buf));
        }

        Ok(None)
    }

    pub(crate) fn redact(&mut self) -> Result<()> {
        if self.written_so_far > 0 {
            bail!("can't redact - already wrote {} bytes", self.written_so_far);
        }

        self.state.write_chunk(
            redacted::REDACTED_CONTENT.to_vec().into(),
            self.expected_hash,
            true,
        )?;

        self.redacted = true;

        Ok(())
    }

    fn finish(mut self) -> Result<(Sha256, StreamingState)> {
        if !self.redacted && self.written_so_far < self.size {
            bail!(
                "not enough data written: {} < {}",
                self.written_so_far,
                self.size
            );
        }

        if let StreamingState::File(file) = &mut self.state {
            file.sync_all()?;
        }

        Ok((self.expected_hash, self.state))
    }
}

pub(crate) enum LfsStoreEntry {
    PointerOnly(LfsPointersEntry),
    PointerAndBlob(LfsPointersEntry, Blob),
}

impl LfsStore {
    fn new(pointers: LfsPointersStore, blobs: LfsBlobsStore) -> Result<Self> {
        Ok(Self { pointers, blobs })
    }

    /// Create a new permanent `LfsStore`.
    ///
    /// Permanent stores will `fsync(2)` data to disk, and will never rotate data out of the store.
    pub fn permanent(path: impl AsRef<Path>, config: &dyn Config) -> Result<Self> {
        let path = path.as_ref();
        let pointers = LfsPointersStore::permanent(path, config)?;
        let blobs = LfsBlobsStore::loose_objects(path)?;
        LfsStore::new(pointers, blobs)
    }

    /// Create a new rotated `LfsStore`.
    pub fn rotated(path: impl AsRef<Path>, config: &dyn Config) -> Result<Self> {
        let path = path.as_ref();
        let pointers = LfsPointersStore::rotated(path, config)?;
        let blobs = LfsBlobsStore::rotated_or_loose_objects(path, config)?;
        LfsStore::new(pointers, blobs)
    }

    /// Open a critical section where cache writes are guaranteed to be present on subsequent read
    /// (if underlying store is indexedlog RotateLog).
    pub(crate) fn with_consistent_reads(&self) -> Option<ConsistentReadGuard> {
        if let LfsBlobsStore::IndexedLog(store) = &self.blobs {
            store.inner.write().with_consistent_reads()
        } else {
            None
        }
    }

    pub fn repair(path: impl AsRef<Path>) -> Result<String> {
        let path = path.as_ref();
        let mut repair_str = String::new();

        repair_str += &LfsPointersStore::repair(get_lfs_pointers_path(path)?)?;
        repair_str += &LfsIndexedLogBlobsStore::repair(get_lfs_blobs_path(path)?)?;

        Ok(repair_str)
    }

    fn blob_impl(&self, key: StoreKey) -> Result<StoreResult<(LfsPointersEntry, Bytes)>> {
        let pointer = self.pointers.entry(&key)?;

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
                        Some(blob) => Ok(StoreResult::Found((entry, blob.into_bytes()))),
                    }
                }
            },
        }
    }

    // TODO(meyer): This is a crappy name, albeit so is blob_impl.
    /// Fetch whatever content is available for the specified StoreKey. Like blob_impl above, but returns just
    /// the LfsPointersEntry when that's all that's found. Mostly copy-pasted from blob_impl above.
    pub(crate) fn fetch_available(
        &self,
        key: &StoreKey,
        ignore_result: bool,
    ) -> Result<Option<LfsStoreEntry>> {
        let pointer = self.pointers.entry(key)?;

        match pointer {
            None => Ok(None),
            Some(entry) => match entry.content_hashes.get(&ContentHashType::Sha256) {
                // TODO(meyer): The docs for LfsPointersEntry say Sha256 will always be available.
                // if it isn't, then should we bother returning the PointerOnly success, panic or return an error,
                // or return NotFound like blob_impl?
                None => Ok(Some(LfsStoreEntry::PointerOnly(entry))),
                Some(content_hash) => {
                    if ignore_result {
                        match self.blobs.contains(&content_hash.clone().unwrap_sha256())? {
                            false => Ok(Some(LfsStoreEntry::PointerOnly(entry))),
                            // Insert stub entry since the result doesn't matter.
                            true => Ok(Some(LfsStoreEntry::PointerAndBlob(
                                entry,
                                Bytes::new().into(),
                            ))),
                        }
                    } else {
                        match self
                            .blobs
                            .get(&content_hash.clone().unwrap_sha256(), entry.size)?
                        {
                            None => Ok(Some(LfsStoreEntry::PointerOnly(entry))),
                            Some(blob) => Ok(Some(LfsStoreEntry::PointerAndBlob(entry, blob))),
                        }
                    }
                }
            },
        }
    }

    /// Directly get the local content. Do not ask remote servers.
    pub(crate) fn get_local_content_direct(&self, id: &HgId) -> Result<Option<Blob>> {
        let pointer = match self.pointers.get_by_hgid(id)? {
            None => return Ok(None),
            Some(v) => v,
        };
        let hash = match pointer.content_hashes.get(&ContentHashType::Sha256) {
            None => return Ok(None),
            Some(v) => v,
        };
        self.blobs.get(hash.sha256_ref(), pointer.size)
    }

    pub fn get_blob(&self, hash: &Sha256, size: u64) -> Result<Option<Blob>> {
        self.blobs.get(hash, size)
    }

    pub fn add_blob(&self, hash: &Sha256, blob: Bytes) -> Result<()> {
        self.blobs.add(hash, blob)
    }

    pub(crate) fn add_blob_and_pointer(&self, key: Key, bytes: Bytes) -> Result<()> {
        let (lfs_pointer, lfs_blob) = lfs_from_hg_file_blob(key.hgid, &bytes)?;
        let sha256 = lfs_pointer.sha256();
        self.blobs.add(&sha256, lfs_blob)?;
        self.add_pointer(lfs_pointer)
    }

    pub(crate) fn add_pointer(&self, pointer_entry: LfsPointersEntry) -> Result<()> {
        self.pointers.add(pointer_entry)
    }

    pub fn flush(&self) -> Result<()> {
        self.blobs.flush()?;
        self.pointers.0.flush()?;
        Ok(())
    }
}

pub(crate) fn content_header_from_pointer(entry: &LfsPointersEntry) -> Bytes {
    if let Some(copy_from) = &entry.copy_from {
        let copy_from_path: &[u8] = copy_from.path.as_ref();
        let mut ret = Vec::with_capacity(copy_from_path.len() + 21);
        ret.extend_from_slice(b"\x01\n");
        ret.extend_from_slice(b"copy: ");
        ret.extend_from_slice(copy_from_path);
        ret.extend_from_slice(b"\n");
        ret.extend_from_slice(b"copyrev: ");
        ret.extend_from_slice(copy_from.hgid.to_hex().as_bytes());
        ret.extend_from_slice(b"\n");
        ret.extend_from_slice(b"\x01\n");
        ret.into()
    } else {
        Bytes::default()
    }
}

/// When a file was copied, Mercurial expects the blob that the store returns to contain this copy
/// information
pub(crate) fn rebuild_metadata(data: Bytes, entry: &LfsPointersEntry) -> Bytes {
    let header = content_header_from_pointer(entry);
    if !header.is_empty() {
        let mut ret = Vec::with_capacity(data.len() + header.len());
        ret.extend_from_slice(header.as_ref());
        ret.extend_from_slice(data.as_ref());
        ret.into()
    } else if data.as_ref().starts_with(b"\x01\n") {
        let mut ret = Vec::with_capacity(data.len() + 4);
        ret.extend_from_slice(b"\x01\n\x01\n");
        ret.extend_from_slice(data.as_ref());
        ret.into()
    } else {
        data
    }
}

/// Computes an LfsPointersEntry and LFS content Blob from a Mercurial file blob.
pub(crate) fn lfs_from_hg_file_blob(
    hgid: HgId,
    raw_content: &Bytes,
) -> Result<(LfsPointersEntry, Bytes)> {
    let (data, copy_from) = strip_file_metadata(raw_content, SerializationFormat::Hg)?;
    let pointer = LfsPointersEntry::from_file_content(hgid, &data, copy_from)?;
    Ok((pointer, data))
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

impl LfsStore {
    fn blob(&self, key: StoreKey) -> Result<StoreResult<Bytes>> {
        match self.blob_impl(key)? {
            StoreResult::Found((_, blob)) => Ok(StoreResult::Found(blob)),
            StoreResult::NotFound(key) => Ok(StoreResult::NotFound(key)),
        }
    }

    pub fn metadata(&self, key: StoreKey) -> Result<StoreResult<ContentMetadata>> {
        let pointer = self.pointers.entry(&key)?;

        match pointer {
            None => Ok(StoreResult::NotFound(key)),
            Some(pointer_entry) => Ok(StoreResult::Found(pointer_entry.into())),
        }
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
        LfsPointersEntry::from_str(data, hgid)
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
            } else if let Some(suffix) = line.strip_prefix(LFS_POINTER_OID_SHA256) {
                hash = Some(suffix.parse::<Sha256>()?);
            } else if let Some(suffix) = line.strip_prefix(LFS_POINTER_SIZE) {
                size = Some(suffix.parse::<usize>()?);
            } else if let Some(suffix) = line.strip_prefix(LFS_POINTER_X_HG_COPY) {
                path = Some(RepoPath::from_str(suffix)?.to_owned());
            } else if let Some(suffix) = line.strip_prefix(LFS_POINTER_X_HG_COPYREV) {
                copy_hgid = Some(HgId::from_str(suffix)?);
            } else if let Some(suffix) = line.strip_prefix(LFS_POINTER_X_IS_BINARY) {
                is_binary = suffix.parse::<u8>()? == 1;
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

impl LfsRemote {
    pub fn from_config(config: &dyn Config) -> Result<Self> {
        let mut url: String = config.must_get("lfs", "url")?;
        // A trailing '/' needs to be present so that `Url::join` doesn't remove the reponame
        // present at the end of the config.
        url.push('/');
        let url = Url::parse(&url)?;

        if url.scheme() == "file" {
            let path = url.to_file_path().unwrap();
            create_dir(&path)?;
            let file = LfsBlobsStore::loose(path);
            Ok(Self::File(file))
        } else {
            if !["http", "https"].contains(&url.scheme()) {
                bail!("Unsupported url: {}", url);
            }

            let user_agent = config.get_or("experimental", "lfs.user-agent", || {
                format!("Sapling/{}", ::version::VERSION)
            })?;

            let concurrent_fetches = config.get_or("lfs", "concurrentfetches", || 30)?;

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
                "2" => {
                    if !curl::Version::get().feature_http2() {
                        warn!(
                            "Asked to use HTTP/2 but HTTP/2 not available in current build; falling back to 1.1"
                        );
                        HttpVersion::V11
                    } else {
                        HttpVersion::V2
                    }
                }
                x => bail!("Unsupported http_version: {}", x),
            };

            let low_speed_grace_period =
                Duration::from_millis(config.get_or("lfs", "low-speed-grace-period", || 10_000)?);
            let low_speed_min_bytes_per_second =
                config.get_opt::<u32>("lfs", "low-speed-min-bytes-per-second")?;
            let min_transfer_speed =
                low_speed_min_bytes_per_second.map(|min_bytes_per_second| MinTransferSpeed {
                    min_bytes_per_second,
                    window: low_speed_grace_period,
                });

            let download_chunk_size =
                config.get_or::<ByteCount>("lfs", "download-chunk-size", || {
                    (5 * 1024 * 1024).into()
                })?;
            let download_chunk_size = NonZeroU64::new(download_chunk_size.value())
                .context("download chunk size cannot be 0")?;

            let client = http_client("lfs", http_config(config, &url)?);

            Ok(Self::Http(HttpLfsRemote {
                url,
                client: Arc::new(client),
                concurrent_fetches,
                download_chunk_size,
                buf_pool: LimitedBufferPool::new(concurrent_fetches),
                http_options: Arc::new(HttpOptions {
                    accept_zstd,
                    http_version,
                    min_transfer_speed,
                    user_agent,
                    backoff_times,
                    throttle_backoff_times,
                    request_timeout,
                }),
            }))
        }
    }

    /// Legacy API that reads entire LFS object into memory, passing to callback.
    /// Prefer using LfsClient::batch_fetch.
    pub fn batch_fetch(
        &self,
        fctx: FetchContext,
        objs: &HashSet<(Sha256, usize)>,
        mut write_to_store: impl FnMut(Sha256, Bytes) -> Result<()>,
        error_handler: impl FnMut(Sha256, Error),
    ) -> Result<()> {
        let read_from_store = |_sha256, _size| unreachable!();
        let make_inserter = |hash, size| Ok(StreamingInserter::memory(hash, size));
        let done_cb = |hash, state| match state {
            StreamingState::Memory(buf) => write_to_store(hash, buf.into()),
            _ => bail!("unexpected StreamingState"),
        };
        match self {
            LfsRemote::Http(http) => Self::batch_http(
                Some(fctx),
                http,
                objs,
                Operation::Download,
                read_from_store,
                make_inserter,
                done_cb,
                error_handler,
            ),
            LfsRemote::File(file) => Self::batch_fetch_file(file, objs, write_to_store),
        }
    }

    pub fn batch_upload(
        &self,
        objs: &HashSet<(Sha256, usize)>,
        read_from_store: impl Fn(Sha256, u64) -> Result<Option<Bytes>> + Send + Clone + 'static,
        error_handler: impl FnMut(Sha256, Error),
    ) -> Result<()> {
        let make_inserter = |_, _| unreachable!();
        let done = |_, _| unreachable!();
        match self {
            LfsRemote::Http(http) => Self::batch_http(
                None,
                http,
                objs,
                Operation::Upload,
                read_from_store,
                make_inserter,
                done,
                error_handler,
            ),
            LfsRemote::File(file) => Self::batch_upload_file(file, objs, read_from_store),
        }
    }

    async fn send_with_retry(
        fctx: Option<FetchContext>,
        client: Arc<HttpClient>,
        method: Method,
        url: Url,
        add_extra: impl Fn(Request) -> Request,
        check_status: impl Fn(StatusCode) -> Result<(), TransferError>,
        http_options: Arc<HttpOptions>,
        mut chunk_buf: Option<Vec<u8>>,
    ) -> Result<Bytes, FetchError> {
        let span = trace_span!("LfsRemote::send_with_retry", url = %url);

        let host_str = url.host_str().expect("No host in url").to_string();

        async move {
            let mut backoff = http_options.backoff_times.iter().copied();
            let mut throttle_backoff = http_options.throttle_backoff_times.iter().copied();
            let mut attempt = 0;

            let mut seen_error_codes = HashSet::new();

            loop {
                attempt += 1;

                let mut chunk_buf = chunk_buf.take();

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

                if http_options.accept_zstd {
                    req.set_accept_encoding([Encoding::Zstd]);
                }

                if let Some(mts) = http_options.min_transfer_speed {
                    req.set_min_transfer_speed(mts);
                }

                req.set_fetch_cause(fctx.as_ref().map(|fctx| fctx.cause().to_str().to_string()));

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
                    let mut chunks: Vec<Vec<u8>> = Vec::new();
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

                        if let Some(buf) = chunk_buf.as_mut() {
                            buf.extend_from_slice(&chunk);
                        } else {
                            chunks.push(chunk);
                        }
                    }

                    if let Some(buf) = chunk_buf {
                        Result::<_, TransferError>::Ok(buf.into())
                    } else {
                        Result::<_, TransferError>::Ok(join_chunks(&chunks))
                    }
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
                        RetryStrategy::from_http_error(http_error)
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
        fctx: Option<FetchContext>,
        http: &HttpLfsRemote,
        objects: Vec<RequestObject>,
        operation: Operation,
    ) -> Result<Option<ResponseBatch>> {
        let span = info_span!("LfsRemote::send_batch_inner");
        let _guard = span.enter();

        let batch = RequestBatch {
            operation,
            transfers: vec![Default::default()],
            r#ref: None,
            objects,
        };

        let batch_json = serde_json::to_string(&batch)?;

        let batch_url = http.url.join("objects/batch")?;

        let response_fut = async move {
            LfsRemote::send_with_retry(
                fctx.clone(),
                http.client.clone(),
                Method::Post,
                batch_url,
                move |builder| builder.body(batch_json.clone()),
                |_| Ok(()),
                http.http_options.clone(),
                None,
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
        LfsRemote::send_with_retry(
            None,
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
            None,
        )
        .await?;

        Ok(())
    }

    async fn process_download(
        fctx: Option<FetchContext>,
        client: Arc<HttpClient>,
        chunk_size: NonZeroU64,
        action: ObjectAction,
        size: u64,
        http_options: Arc<HttpOptions>,
        mut inserter: StreamingInserter,
        buf_pool: LimitedBufferPool,
    ) -> Result<(Sha256, StreamingState)> {
        let url = Url::from_str(&action.href.to_string())?;

        let chunk_increment = chunk_size.get() - 1;

        let mut chunk_start = 0;

        let file_end = size.saturating_sub(1);

        // Recyclable buffer we use for each chunk.
        let (pool_handle, mut buf) = buf_pool.get().await;

        buf.reserve(chunk_size.get().min(size) as usize);

        let mut buf = Some(buf);

        while chunk_start < file_end {
            let chunk_end = std::cmp::min(file_end, chunk_start + chunk_increment);
            let range = format!("bytes={}-{}", chunk_start, chunk_end);

            let chunk_res = LfsRemote::send_with_retry(
                fctx.clone(),
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

                    // 200 is okay if we requested the entire file in one chunk.
                    if chunk_start == 0 && chunk_end == file_end && status == http::StatusCode::OK {
                        return Ok(());
                    }

                    Err(TransferError::UnexpectedHttpStatus {
                        expected: http::StatusCode::PARTIAL_CONTENT,
                        received: status,
                    })
                },
                http_options.clone(),
                buf,
            )
            .await;

            match chunk_res {
                Ok(chunk) => {
                    // add_chunk() is blocking and quite slow (sha256 hash and indexedlog append).
                    // It slows down the async execution by more than 50% - we must spawn_blocking.
                    (inserter, buf) = spawn_blocking(move || {
                        let buf = inserter.add_chunk(chunk)?;
                        Ok::<_, Error>((inserter, buf))
                    })
                    .await??;
                }
                Err(err) => match err.error {
                    TransferError::HttpStatus(http::StatusCode::GONE, _) => {
                        inserter.redact()?;
                        return inserter.finish();
                    }
                    _ => return Err(err.into()),
                },
            };

            chunk_start = chunk_end + 1;
        }

        if let Some(buf) = buf {
            pool_handle.put(buf);
        }

        inserter.finish()
    }

    /// Fetch and Upload blobs from the LFS server.
    ///
    /// When uploading, the `write_to_store` is guaranteed not to be called, similarly when fetching,
    /// the `read_from_store` will not be called.
    ///
    /// The protocol is described at: https://github.com/git-lfs/git-lfs/blob/master/docs/api/batch.md
    fn batch_http(
        fctx: Option<FetchContext>,
        http: &HttpLfsRemote,
        objs: &HashSet<(Sha256, usize)>,
        operation: Operation,
        read_from_store: impl Fn(Sha256, u64) -> Result<Option<Bytes>> + Send + Clone + 'static,
        mut make_inserter: impl FnMut(Sha256, u64) -> Result<StreamingInserter>,
        mut done_cb: impl FnMut(Sha256, StreamingState) -> Result<()>,
        mut error_handler: impl FnMut(Sha256, Error),
    ) -> Result<()> {
        let objs = objs
            .iter()
            .map(|(oid, size)| RequestObject {
                oid: LfsSha256(oid.into_inner()),
                size: *size as u64,
            })
            .collect::<Vec<_>>();

        let response = LfsRemote::send_batch_request(fctx.clone(), http, objs, operation)?;

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

            for (op, action) in actions.into_iter().flat_map(|h| h.into_iter()) {
                let oid = Sha256::from(oid.0);

                let fut = match op {
                    Operation::Upload => LfsRemote::process_upload(
                        http.client.clone(),
                        action,
                        oid,
                        object.object.size,
                        read_from_store.clone(),
                        http.http_options.clone(),
                    )
                    .map(|res| match res {
                        Ok(()) => None,
                        Err(err) => Some(Err(err)),
                    })
                    .left_future(),
                    Operation::Download => LfsRemote::process_download(
                        fctx.clone(),
                        http.client.clone(),
                        http.download_chunk_size,
                        action,
                        object.object.size,
                        http.http_options.clone(),
                        make_inserter(oid, object.object.size)?,
                        http.buf_pool.clone(),
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
            let (sha, state) = result?;
            done_cb(sha, state)?;
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
                write_to_store(*hash, data.into_bytes())?;
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

impl LfsClient {
    pub fn new(
        shared: Arc<LfsStore>,
        local: Option<Arc<LfsStore>>,
        config: &dyn Config,
    ) -> Result<Self> {
        let move_after_upload = config.get_or("lfs", "moveafterupload", || false)?;

        Ok(Self {
            shared,
            local,
            move_after_upload,
            remote: Arc::new(LfsRemote::from_config(config)?),
        })
    }

    /// Fetch specified objects from remote server, saving results into shared cache. Results are
    /// streamed into shared cache, skipping only the final chunk if there is a content hash
    /// mismatch.
    pub fn batch_fetch(
        &self,
        fctx: FetchContext,
        objs: &HashSet<(Sha256, usize)>,
        mut done_cb: impl FnMut(Sha256),
        error_handler: impl FnMut(Sha256, Error),
    ) -> Result<()> {
        match self.remote.as_ref() {
            LfsRemote::Http(http) => {
                let make_inserter =
                    |hash, size| StreamingInserter::new(&self.shared.blobs, hash, size);

                let done_cb = |hash: Sha256, _state: StreamingState| Ok(done_cb(hash));

                LfsRemote::batch_http(
                    Some(fctx),
                    http,
                    objs,
                    Operation::Download,
                    |_sha256, _size| unreachable!(),
                    make_inserter,
                    done_cb,
                    error_handler,
                )
            }

            LfsRemote::File(file) => LfsRemote::batch_fetch_file(file, objs, |hash, bytes| {
                self.shared.add_blob(&hash, bytes)?;
                done_cb(hash);
                Ok(())
            }),
        }
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

    pub fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let local_store = match self.local.as_ref() {
            None => return Ok(keys.to_vec()),
            Some(local) => local,
        };

        let mut not_found = Vec::new();

        let objs = keys
            .iter()
            .map(|k| {
                if let Some(pointer) = local_store.pointers.entry(k)? {
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
            let span = info_span!("LfsClient::upload", num_blobs = objs.len(), size = &0);
            let _guard = span.enter();

            let size = Arc::new(AtomicUsize::new(0));

            self.batch_upload(
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

            span.record("size", size.load(Ordering::Relaxed));
        }

        if self.move_after_upload {
            let span = info_span!("LfsClient::move_after_upload");
            let _guard = span.enter();
            // All the blobs were successfully uploaded, we can move the blobs from the local store
            // to the shared store. This is safe to do as blobs will never be collected from the
            // server once uploaded.
            for obj in objs {
                move_blob(&obj.0, obj.1 as u64, local_store, &self.shared)?;
            }
        }

        Ok(not_found)
    }

    pub(crate) fn with_shared_only(&self) -> Self {
        let mut c = self.clone();
        c.local = Some(self.shared.clone());
        c
    }

    pub(crate) fn flush(&self) -> Result<()> {
        let mut res = Ok(());

        if let Some(err) = self.local.as_ref().and_then(|l| l.flush().err()) {
            res = Err(err);
        }

        if let Some(err) = self.shared.flush().err() {
            res = Err(err);
        }

        if let LfsRemote::Http(http) = self.remote.as_ref() {
            // Try to drop any big buffers we are holding on to.
            http.buf_pool.gc();
        }

        res
    }
}

/// Move a blob contained in `from` to the store `to`.
///
/// After this succeeds, the blob's lifetime will be similar to any shared blob, it is the caller's
/// responsibility to ensure that the blob can be fetched from the LFS server.
fn move_blob(hash: &Sha256, size: u64, from: &LfsStore, to: &LfsStore) -> Result<()> {
    (|| {
        let blob = from
            .blobs
            .get(hash, size)?
            .ok_or_else(|| format_err!("Cannot find blob for {}", hash))?;

        to.blobs.add(hash, blob.into_bytes())?;
        from.blobs.remove(hash)?;

        (|| -> Result<()> {
            let key = StoreKey::from(ContentHash::Sha256(*hash));
            if let Some(pointer) = from.pointers.entry(&key)? {
                to.pointers.add(pointer)?
            }
            Ok(())
        })()
        .with_context(|| format!("Cannot move pointer for {}", hash))
    })()
    .with_context(|| format!("Cannot move blob {}", hash))
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

/// LimitedBufferPool acts both as a pool to reuse Vec<u8> buffers, and as a request limiter
/// to only allow N concurrent requests at once.
#[derive(Clone)]
struct LimitedBufferPool {
    tx: flume::Sender<(Vec<u8>, Instant)>,
    rx: flume::Receiver<(Vec<u8>, Instant)>,
    gc_timeout: Duration,
}

impl LimitedBufferPool {
    pub(crate) fn new(max: usize) -> Self {
        let (tx, rx) = flume::bounded(max);

        let pool = Self {
            tx,
            rx,
            gc_timeout: Duration::from_secs(60),
        };

        for _ in 0..max {
            pool.put(Vec::new());
        }

        pool
    }

    /// Get a PoolHandle and a buffer. You should call `handle.put(buf)` when you are done with the
    /// buf.
    pub(crate) async fn get(&self) -> (PoolHandle, Vec<u8>) {
        let (mut buf, _) = self.rx.recv_async().await.unwrap();
        buf.clear();

        (
            PoolHandle {
                pool: self.clone(),
                returned: AtomicBool::new(false),
            },
            buf,
        )
    }

    /// Internal method - put buf back.
    fn put(&self, buf: Vec<u8>) {
        self.tx.try_send((buf, Instant::now())).ok();
    }

    /// Drop buffers that haven't been used in the last 1 minute.
    pub(crate) fn gc(&self) {
        // Bound our iterations to capacity to be extra sure we can't get stuck looping.
        for _ in 0..self.tx.capacity().unwrap_or_default() {
            // Iterate through buffers, oldest first.
            if let Ok((buf, ts)) = self.rx.try_recv() {
                if ts.elapsed() < self.gc_timeout {
                    // Buffer was used in last minute - put it back and stop gc().
                    self.put(buf);
                    break;
                } else {
                    // Buffer wasn't used in last minute - drop it and put an empty buffer back in.
                    if buf.capacity() > 0 {
                        tracing::trace!("dropping LFS buffer of capacity {}", buf.capacity());
                    }
                    self.put(Vec::new());
                }
            }
        }
    }
}

struct PoolHandle {
    pool: LimitedBufferPool,
    returned: AtomicBool,
}

impl Drop for PoolHandle {
    /// User didn't call PoolHandle::put() - release the spot in pool even though we don't have a
    /// buffer to return.
    fn drop(&mut self) {
        if !self.returned.swap(true, Ordering::AcqRel) {
            self.pool.put(Vec::new());
        }
    }
}

impl PoolHandle {
    /// Put buf back in pool for reuse.
    pub(crate) fn put(self, buf: Vec<u8>) {
        if !self.returned.swap(true, Ordering::AcqRel) {
            self.pool.put(buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use parking_lot::Mutex;
    use quickcheck::quickcheck;
    use storemodel::SerializationFormat;
    use tempfile::TempDir;
    use types::testutil::*;

    use super::*;
    #[cfg(feature = "fb")]
    use crate::testutil::TestBlob;
    use crate::testutil::example_blob;
    #[cfg(feature = "fb")]
    use crate::testutil::example_blob2;
    use crate::testutil::get_lfs_batch_mock;
    use crate::testutil::get_lfs_download_mock;
    use crate::testutil::make_lfs_config;
    #[cfg(feature = "fb")]
    use crate::testutil::nonexistent_blob;
    use crate::testutil::setconfig;

    #[test]
    fn test_new_rotated() -> Result<()> {
        let dir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &dir, "test_new_shared");
        let _ = LfsStore::rotated(&dir, &config)?;

        let mut lfs_dir = dir.as_ref().to_owned();
        lfs_dir.push("lfs");
        lfs_dir.push("objects");
        assert!(lfs_dir.is_dir());

        Ok(())
    }

    #[test]
    fn test_new_permanent() -> Result<()> {
        let dir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &dir, "test_new_local");
        let _ = LfsStore::permanent(&dir, &config)?;

        let mut lfs_dir = dir.as_ref().to_owned();
        lfs_dir.push("lfs");
        lfs_dir.push("objects");
        assert!(lfs_dir.is_dir());

        Ok(())
    }

    #[test]
    fn test_add() -> Result<()> {
        let dir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &dir, "test_add");
        let store = LfsStore::rotated(&dir, &config)?;

        let k1 = key("a", "2");

        let data = Bytes::from(&[1, 2, 3, 4][..]);
        store.add_blob_and_pointer(k1, data.clone())?;
        store.flush()?;

        let indexedlog_blobs = LfsIndexedLogBlobsStore::rotated(dir.path(), &config)?;
        let hash = ContentHash::sha256(&data).unwrap_sha256();

        assert!(indexedlog_blobs.contains(&hash)?);

        assert_eq!(
            Some(data.clone().into()),
            indexedlog_blobs.get(&hash, data.len() as u64)?
        );

        Ok(())
    }

    #[test]
    fn test_loose() -> Result<()> {
        let dir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &dir, "test_loose");
        let blob_store = LfsBlobsStore::rotated_or_loose_objects(dir.path(), &config)?;
        let loose_store = LfsBlobsStore::loose(get_lfs_objects_path(dir.path())?);

        let data = Bytes::from(&[1, 2, 3, 4][..]);
        let sha256 = ContentHash::sha256(&data).unwrap_sha256();
        loose_store.add(&sha256, data.clone())?;

        assert!(blob_store.contains(&sha256)?);
        assert_eq!(
            blob_store.get(&sha256, data.len() as u64)?,
            Some(data.into())
        );

        Ok(())
    }

    #[test]
    fn test_add_get() -> Result<()> {
        let dir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &dir, "test_add_get");
        let store = LfsStore::rotated(&dir, &config)?;

        let k1 = key("a", "2");
        let data = Bytes::from(&[1, 2, 3, 4][..]);

        store.add_blob_and_pointer(k1.clone(), data.clone())?;

        let stored = store.blob(StoreKey::hgid(k1))?;
        assert_eq!(StoreResult::Found(data), stored);

        Ok(())
    }

    #[test]
    fn test_invalid_hash() -> Result<()> {
        let dir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &dir, "test_invalid_hash");

        let store = LfsIndexedLogBlobsStore::rotated(dir.path(), &config)?;

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
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &dir, "test_invalid_hash");

        let store = LfsIndexedLogBlobsStore::rotated(dir.path(), &config)?;

        let data = Bytes::from_static(b"data");
        let hash = ContentHash::sha256(&data).unwrap_sha256();

        // Insert some poisoned chunks under the same hash.
        store.inner.append(serialize(&LfsIndexedLogBlobsEntry {
            sha256: hash.clone(),
            range: (0..2),
            data: Bytes::from_static(b"oo"),
        })?)?;
        store.inner.append(serialize(&LfsIndexedLogBlobsEntry {
            sha256: hash.clone(),
            range: (2..4),
            data: Bytes::from_static(b"ps"),
        })?)?;

        // Insert the new, correct data.
        store.add(&hash, data.clone())?;

        store.flush()?;

        // Make sure we get the new data.
        assert_eq!(store.get(&hash, data.len() as u64)?, Some(data.into()));

        Ok(())
    }

    #[test]
    fn test_add_get_split() -> Result<()> {
        let dir = TempDir::new()?;
        let server = mockito::Server::new();
        let mut config = make_lfs_config(&server, &dir, "test_add_get_split");
        setconfig(&mut config, "lfs", "blobschunksize", "2");

        let store = LfsStore::rotated(&dir, &config)?;

        let k1 = key("a", "2");
        let data = Bytes::from(&[1, 2, 3, 4][..]);

        store.add_blob_and_pointer(k1.clone(), data.clone())?;
        let k = StoreKey::hgid(k1);
        let stored = store.blob(k.clone())?;
        assert_eq!(StoreResult::Found(data.clone()), stored);

        store.flush()?;

        let stored = store.blob(k)?;
        assert_eq!(StoreResult::Found(data), stored);

        Ok(())
    }

    #[test]
    fn test_partial_blob() -> Result<()> {
        let dir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &dir, "test_partial_blob");

        let store = LfsIndexedLogBlobsStore::rotated(dir.path(), &config)?;

        let data = Bytes::from(&[1, 2, 3, 4][..]);
        let partial = data.slice(2..);
        let sha256 = ContentHash::sha256(&data).unwrap_sha256();

        let entry = LfsIndexedLogBlobsEntry {
            sha256: sha256.clone(),
            range: Range { start: 2, end: 4 },
            data: partial,
        };

        store.inner.append(serialize(&entry)?)?;
        store.flush()?;

        assert_eq!(store.get(&sha256, data.len() as u64)?, None);

        Ok(())
    }

    #[test]
    fn test_full_chunked() -> Result<()> {
        let dir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &dir, "test_full_chunked");

        let store = LfsIndexedLogBlobsStore::rotated(dir.path(), &config)?;

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
        store.inner.append(serialize(&first_entry)?)?;

        let second_entry = LfsIndexedLogBlobsEntry {
            sha256: sha256.clone(),
            range: Range { start: 1, end: 4 },
            data: second,
        };
        store.inner.append(serialize(&second_entry)?)?;

        let last_entry = LfsIndexedLogBlobsEntry {
            sha256: sha256.clone(),
            range: Range { start: 4, end: 7 },
            data: last,
        };
        store.inner.append(serialize(&last_entry)?)?;

        store.flush()?;

        assert_eq!(store.get(&sha256, data.len() as u64)?, Some(data.into()));

        Ok(())
    }

    #[test]
    fn test_overlapped_chunked() -> Result<()> {
        let dir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &dir, "test_overlapped_chunked");

        let store = LfsIndexedLogBlobsStore::rotated(dir.path(), &config)?;

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
        store.inner.append(serialize(&first_entry)?)?;

        let second_entry = LfsIndexedLogBlobsEntry {
            sha256: sha256.clone(),
            range: Range { start: 2, end: 3 },
            data: second,
        };
        store.inner.append(serialize(&second_entry)?)?;

        let last_entry = LfsIndexedLogBlobsEntry {
            sha256: sha256.clone(),
            range: Range { start: 2, end: 7 },
            data: last,
        };
        store.inner.append(serialize(&last_entry)?)?;

        store.flush()?;

        assert_eq!(store.get(&sha256, data.len() as u64)?, Some(data.into()));

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
            let (without, copy) = strip_file_metadata(&with_metadata, SerializationFormat::Hg)?;

            Ok(data == without && copy == copy_from)
        }
    }

    #[test]
    fn test_add_get_copyfrom() -> Result<()> {
        let dir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &dir, "test_add_get_copyform");
        let store = LfsStore::rotated(&dir, &config)?;

        let k1 = key("a", "2");
        let data = Bytes::copy_from_slice(
            format!(
                "\x01\ncopy: {}\ncopyrev: {}\n\x01\nthis is a blob",
                k1.path, k1.hgid
            )
            .as_bytes(),
        );

        store.add_blob_and_pointer(k1.clone(), data.clone())?;
        let stored = store.blob(StoreKey::hgid(k1))?;
        assert_eq!(StoreResult::Found(Bytes::from("this is a blob")), stored);

        Ok(())
    }

    #[allow(unexpected_cfgs)]
    #[cfg(feature = "fb")]
    mod fb_test {
        use std::collections::BTreeMap;
        use std::env::set_var;
        use std::sync::atomic::AtomicBool;

        #[cfg(fbcode_build)]
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

            fn as_callback(
                &self,
            ) -> impl Fn(Sha256, Bytes) -> Result<()> + Send + Clone + 'static + use<> {
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
            let mut server = mockito::Server::new();
            let config = make_lfs_config(&server, &cachedir, "test_lfs_proxy_non_present");

            let blob = &example_blob();
            let _m1 = get_lfs_batch_mock(&mut server, 200, &[blob]);

            let _m2 = get_lfs_download_mock(&mut server, 200, blob);

            let lfs = Arc::new(LfsStore::rotated(&lfsdir, &config)?);
            let remote = LfsClient::new(lfs, None, &config)?;

            let objs = [(blob.sha, blob.size)]
                .iter()
                .cloned()
                .collect::<HashSet<_>>();
            remote.remote.batch_fetch(
                FetchContext::default(),
                &objs,
                sentinel.as_callback(),
                |_, _| {},
            )?;
            assert!(sentinel.get());

            Ok(())
        }

        #[test]
        #[cfg(not(windows))]
        fn test_lfs_proxy_no_http() -> Result<()> {
            let _env_lock = crate::env_lock();

            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let server = mockito::Server::new();
            let config = make_lfs_config(&server, &cachedir, "test_lfs_proxy_no_http");

            // TODO: Audit that the environment access only happens in single-threaded code.
            unsafe { set_var("https_proxy", "fwdproxy:8082") };

            let lfs = Arc::new(LfsStore::rotated(&lfsdir, &config)?);
            let remote = LfsClient::new(lfs, None, &config)?;

            let blob = example_blob();
            let objs = [(blob.sha, blob.size)]
                .iter()
                .cloned()
                .collect::<HashSet<_>>();
            let resp = remote.batch_fetch(
                FetchContext::default(),
                &objs,
                |_| unreachable!(),
                |_, _| {},
            );
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
            let server = mockito::Server::new();
            let config = make_lfs_config(&server, &cachedir, "test_lfs_proxy_http");

            // TODO: Audit that the environment access only happens in single-threaded code.
            unsafe { set_var("https_proxy", "http://fwdproxy:8082") };

            let lfs = Arc::new(LfsStore::rotated(&lfsdir, &config)?);
            let remote = LfsClient::new(lfs, None, &config)?;

            let blob = example_blob();
            let objs = [(blob.sha, blob.size)]
                .iter()
                .cloned()
                .collect::<HashSet<_>>();
            let resp = remote.batch_fetch(
                FetchContext::default(),
                &objs,
                |_| unreachable!(),
                |_, _| {},
            );
            assert!(resp.is_err());

            Ok(())
        }

        #[test]
        fn test_lfs_no_proxy() -> Result<()> {
            let _env_lock = crate::env_lock();

            let sentinel = Sentinel::new();
            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let mut server = mockito::Server::new();
            let config = make_lfs_config(&server, &cachedir, "test_lfs_no_proxy");

            let blob = &example_blob();
            let _m1 = get_lfs_batch_mock(&mut server, 200, &[blob]);

            let _m2 = get_lfs_download_mock(&mut server, 200, blob);

            // TODO: Audit that the environment access only happens in single-threaded code.
            unsafe { set_var("http_proxy", "http://shouldnt-touch-this:8082") };
            // TODO: Audit that the environment access only happens in single-threaded code.
            unsafe { set_var("NO_PROXY", "localhost,127.0.0.1") };

            let lfs = Arc::new(LfsStore::rotated(&lfsdir, &config)?);
            let remote = LfsClient::new(lfs, None, &config)?;

            let objs = [(blob.sha, blob.size)]
                .iter()
                .cloned()
                .collect::<HashSet<_>>();
            remote.remote.batch_fetch(
                FetchContext::default(),
                &objs,
                sentinel.as_callback(),
                |_, _| {},
            )?;
            assert!(sentinel.get());

            Ok(())
        }

        fn test_download<C>(
            server: &mut mockito::ServerGuard,
            configure: C,
            blobs: &[&TestBlob],
        ) -> Result<()>
        where
            C: for<'a> FnOnce(&'a mut BTreeMap<String, String>),
        {
            let _env_lock = crate::env_lock();

            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let mut config = make_lfs_config(server, &cachedir, "test_download");
            configure(&mut config);
            let lfs = Arc::new(LfsStore::rotated(&lfsdir, &config)?);
            let remote = LfsClient::new(lfs, None, &config)?;

            let _mocks: Vec<_> = blobs
                .iter()
                .map(|b| get_lfs_download_mock(server, 200, b))
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
            remote.remote.batch_fetch(
                FetchContext::default(),
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

            let mut server = mockito::Server::new();
            let _m1 = get_lfs_batch_mock(&mut server, 200, &blobs);

            test_download(
                &mut server,
                |config| setconfig(config, "lfs", "http-version", "1.1"),
                &blobs,
            )
        }

        #[test]
        fn test_lfs_remote_http2() -> Result<()> {
            if !curl::Version::get().feature_http2() {
                // Skip this test if HTTP/2 is not available for locally built curl
                return Ok(());
            }
            let b1 = example_blob();
            let b2 = example_blob2();
            let b3 = nonexistent_blob();

            let blobs = vec![&b1, &b2, &b3];

            let mut server = mockito::Server::new();
            let _m1 = get_lfs_batch_mock(&mut server, 200, &blobs);

            test_download(
                &mut server,
                |config| setconfig(config, "lfs", "http-version", "2"),
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

            let mut server = mockito::Server::new();
            let _m1 = get_lfs_batch_mock(&mut server, 200, &blobs);

            test_download(
                &mut server,
                |config| {
                    setconfig(config, "lfs", "download-chunk-size", "3");
                },
                &blobs,
            )
        }

        #[test]
        fn test_lfs_invalid_http() -> Result<()> {
            let _env_lock = crate::env_lock();

            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let server = mockito::Server::new();
            let mut config = make_lfs_config(&server, &cachedir, "test_lfs_invalid_http");
            setconfig(&mut config, "lfs", "http-version", "3");

            let lfs = Arc::new(LfsStore::rotated(&lfsdir, &config).unwrap());
            let result = LfsClient::new(lfs, None, &config);

            assert!(result.is_err());

            Ok(())
        }

        #[test]
        fn test_lfs_request_timeout() -> Result<()> {
            let _env_lock = crate::env_lock();

            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let server = mockito::Server::new();
            let mut config = make_lfs_config(&server, &cachedir, "test_lfs_request_timeout");

            setconfig(&mut config, "lfs", "requesttimeout", "0");

            let lfs = Arc::new(LfsStore::rotated(&lfsdir, &config)?);
            let remote = LfsClient::new(lfs, None, &config)?;

            let blob = (
                Sha256::from_str(
                    "fc613b4dfd6736a7bd268c8a0e74ed0d1c04a959f59dd74ef2874983fd443fc9",
                )?,
                6,
                Bytes::from(&b"master"[..]),
            );

            let objs = [(blob.0, blob.1)].iter().cloned().collect::<HashSet<_>>();
            let res = remote.batch_fetch(
                FetchContext::default(),
                &objs,
                |_| unreachable!(),
                |_, _| {},
            );
            assert!(res.is_err());

            Ok(())
        }

        #[cfg(fbcode_build)]
        #[test]
        fn test_lfs_redacted() -> Result<()> {
            let _env_lock = crate::env_lock();

            let cachedir = TempDir::new()?;
            let lfsdir = TempDir::new()?;
            let mut server = mockito::Server::new();
            let mut config = make_lfs_config(&server, &cachedir, "test_lfs_redacted");
            setconfig(
                &mut config,
                "lfs",
                "url",
                &[server.url(), "/repo".to_string()].concat(),
            );

            let blob = &example_blob();

            let _m1 = get_lfs_batch_mock(&mut server, 200, &[blob]);

            let _m2 = server
                .mock("GET", format!("/repo/download/{}", blob.oid).as_str())
                .with_status(410)
                .create();

            let lfs = Arc::new(LfsStore::rotated(&lfsdir, &config)?);
            let remote = LfsClient::new(lfs, None, &config)?;

            let objs = [(blob.sha, blob.size)]
                .iter()
                .cloned()
                .collect::<HashSet<_>>();

            remote.remote.batch_fetch(
                FetchContext::default(),
                &objs,
                |_, data| {
                    assert!(is_redacted(&data.into()));
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
        let server = mockito::Server::new();
        let mut config = make_lfs_config(&server, &cachedir, "test_lfs_remote_file");

        let lfsdir = TempDir::new()?;
        let lfs = Arc::new(LfsStore::rotated(&lfsdir, &config)?);

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
        setconfig(&mut config, "lfs", "url", url.as_str());

        let remote = LfsClient::new(lfs, None, &config)?;

        let objs = [(blob1.0, blob1.1), (blob2.0, blob2.1)]
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let out = Arc::new(Mutex::new(Vec::new()));

        remote.remote.batch_fetch(
            FetchContext::default(),
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
        let server = mockito::Server::new();
        let mut config = make_lfs_config(&server, &cachedir, "test_lfs_upload_remote_file");

        let lfsdir = TempDir::new()?;
        let shared_lfs = Arc::new(LfsStore::rotated(&lfsdir, &config)?);
        let local_lfs = Arc::new(LfsStore::permanent(&lfsdir, &config)?);

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
        setconfig(&mut config, "lfs", "url", url.as_str());

        let remote = LfsClient::new(shared_lfs, Some(local_lfs.clone()), &config)?;

        let objs = [(blob1.0, blob1.1), (blob2.0, blob2.1)]
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        remote.batch_upload(
            &objs,
            move |sha256, size| {
                local_lfs
                    .blobs
                    .get(&sha256, size)
                    .map(|r| r.map(|b| b.into_bytes()))
            },
            |_, _| {},
        )?;

        assert_eq!(
            remote_lfs_file_store.get(&blob1.0, blob1.1 as u64)?,
            Some(blob1.2.into())
        );
        assert_eq!(
            remote_lfs_file_store.get(&blob2.0, blob2.1 as u64)?,
            Some(blob2.2.into())
        );

        Ok(())
    }

    #[test]
    fn test_lfs_upload_move_to_shared() -> Result<()> {
        let _env_lock = crate::env_lock();

        let cachedir = TempDir::new()?;
        let server = mockito::Server::new();
        let mut config = make_lfs_config(&server, &cachedir, "test_lfs_upload_move_to_shared");

        let lfsdir = TempDir::new()?;
        let shared_lfs = Arc::new(LfsStore::rotated(&lfsdir, &config)?);
        let local_lfs = Arc::new(LfsStore::permanent(&lfsdir, &config)?);

        let k1 = key("a", "2");
        let data = Bytes::from("THIS IS A LARGE BLOB");

        local_lfs.add_blob_and_pointer(k1.clone(), data.clone())?;

        let remote_dir = TempDir::new()?;
        let url = Url::from_file_path(&remote_dir).unwrap();
        setconfig(&mut config, "lfs", "url", url.as_str());

        let remote = Arc::new(LfsClient::new(
            shared_lfs.clone(),
            Some(local_lfs.clone()),
            &config,
        )?);
        let k = StoreKey::hgid(k1.clone());
        remote.upload(&[k.clone()])?;

        let contentk = StoreKey::Content(ContentHash::sha256(&data), Some(k1));

        // The blob was moved from the local store to the shared store.
        assert_eq!(local_lfs.blob(k.clone())?, StoreResult::NotFound(contentk));
        assert_eq!(shared_lfs.blob(k)?, StoreResult::Found(data));

        Ok(())
    }

    #[test]
    fn test_blob() -> Result<()> {
        let dir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &dir, "test_blob");
        let store = LfsStore::rotated(&dir, &config)?;

        let data = Bytes::from(&[1, 2, 3, 4][..]);
        let k1 = key("a", "2");

        store.add_blob_and_pointer(k1.clone(), data.clone())?;

        let blob = store.blob(StoreKey::from(k1))?;
        assert_eq!(blob, StoreResult::Found(data));

        Ok(())
    }

    #[test]
    fn test_metadata() -> Result<()> {
        let dir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &dir, "test_metadata");
        let store = LfsStore::rotated(&dir, &config)?;

        let k1 = key("a", "2");
        let data = Bytes::from(&[1, 2, 3, 4][..]);
        let hash = ContentHash::sha256(&data);

        store.add_blob_and_pointer(k1.clone(), data)?;

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

            let mut server = mockito::Server::new();
            let m1 = get_lfs_batch_mock(&mut server, 200, &blobs).expect(1);
            let m2 = get_lfs_download_mock(&mut server, 408, &blob1)
                .pop()
                .unwrap()
                .expect(req_count);
            let cachedir = TempDir::new()?;
            let mut config = make_lfs_config(&server, &cachedir, "test_download");

            setconfig(&mut config, "lfs", "backofftimes", backoff_config);

            let lfsdir = TempDir::new()?;
            let lfs = Arc::new(LfsStore::rotated(&lfsdir, &config)?);
            let remote = LfsClient::new(lfs, None, &config)?;
            let objs = [(blobs[0].sha, blobs[0].size)]
                .iter()
                .cloned()
                .collect::<HashSet<_>>();

            // Make sure we get an error (but don't panic).
            assert!(
                remote
                    .batch_fetch(FetchContext::default(), &objs, |_| (), |_, _| {})
                    .is_err()
            );

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

    #[test]
    fn test_streaming_inserter() -> Result<()> {
        let dir = TempDir::new()?;
        let config = BTreeMap::<&str, &str>::new();

        let store =
            LfsBlobsStore::IndexedLog(LfsIndexedLogBlobsStore::rotated(dir.path(), &config)?);

        let data = Bytes::from_static(b"abc");
        let expected_hash = ContentHash::sha256(&data).unwrap_sha256();

        {
            // Incomplete write
            let mut inserter = StreamingInserter::new(&store, expected_hash, data.len() as u64)?;
            inserter.add_chunk(data.slice(0..1))?;
            inserter.add_chunk(data.slice(1..2))?;
            assert!(inserter.finish().is_err());
            assert!(store.get(&expected_hash, data.len() as u64)?.is_none());
        }

        {
            // Corrupted data
            let mut inserter = StreamingInserter::new(&store, expected_hash, data.len() as u64)?;
            inserter.add_chunk(data.slice(0..1))?;
            inserter.add_chunk(data.slice(1..2))?;
            assert!(inserter.add_chunk(Bytes::from_static(b"z")).is_err());
            assert!(inserter.finish().is_err());
            assert!(store.get(&expected_hash, data.len() as u64)?.is_none());
        }

        {
            // Good
            let mut inserter = StreamingInserter::new(&store, expected_hash, data.len() as u64)?;
            inserter.add_chunk(data.slice(0..1))?;
            inserter.add_chunk(data.slice(1..2))?;
            inserter.add_chunk(data.slice(2..3))?;
            assert!(inserter.finish().is_ok());
            assert_eq!(
                store.get(&expected_hash, data.len() as u64)?.unwrap(),
                data.clone().into()
            );
        }

        {
            // Insert in a single chunk
            let mut inserter = StreamingInserter::new(&store, expected_hash, data.len() as u64)?;
            inserter.add_chunk(data.clone())?;
            assert!(inserter.finish().is_ok());
            assert_eq!(
                store.get(&expected_hash, data.len() as u64)?.unwrap(),
                data.clone().into()
            );
        }

        {
            // Multiple storage chunks.
            let mut idl_store = LfsIndexedLogBlobsStore::rotated(dir.path(), &config)?;
            idl_store.chunk_size = 2;
            let store = LfsBlobsStore::IndexedLog(idl_store);

            let mut inserter = StreamingInserter::new(&store, expected_hash, data.len() as u64)?;
            inserter.add_chunk(data.slice(0..1))?;
            inserter.add_chunk(data.slice(1..2))?;
            inserter.add_chunk(data.slice(2..3))?;
            assert!(inserter.finish().is_ok());
            assert_eq!(
                store.get(&expected_hash, data.len() as u64)?.unwrap(),
                data.clone().into()
            );
        }

        {
            // Multiple owned storage chunks.
            let mut idl_store = LfsIndexedLogBlobsStore::rotated(dir.path(), &config)?;
            idl_store.chunk_size = 2;
            let store = LfsBlobsStore::IndexedLog(idl_store);

            let mut inserter = StreamingInserter::new(&store, expected_hash, data.len() as u64)?;
            inserter.add_chunk(data.slice(0..1).to_vec().into())?;
            inserter.add_chunk(data.slice(1..2).to_vec().into())?;
            inserter.add_chunk(data.slice(2..3).to_vec().into())?;
            assert!(inserter.finish().is_ok());
            assert_eq!(
                store.get(&expected_hash, data.len() as u64)?.unwrap(),
                data.clone().into()
            );
        }

        {
            // Stream to in-memory buffer
            let mut inserter = StreamingInserter::memory(expected_hash, data.len() as u64);
            inserter.add_chunk(data.clone())?;

            let state = inserter.finish()?.1;
            match state {
                StreamingState::Memory(buf) => assert_eq!(buf, data.as_ref()),
                _ => panic!("bad state"),
            }
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_limited_buffer_pool() -> Result<()> {
        let mut pool = LimitedBufferPool::new(2);

        // So buffers are cleared instantly on pool.gc().
        pool.gc_timeout = Duration::ZERO;

        let (handle1, mut buf1) = pool.get().await;
        assert!(buf1.is_empty());

        let (handle2, buf2) = pool.get().await;
        assert!(buf2.is_empty());

        // Pool empty.
        assert!(pool.get().now_or_never().is_none());

        // Use buf2 and then put back.
        buf1.extend_from_slice(b"hello");
        handle1.put(buf1);

        // Now we can get buf1 back.
        let (handle1, buf1) = pool.get().await;
        assert!(buf1.is_empty());
        assert!(buf1.capacity() >= b"hello".len());

        // Oops - forgot to put back buf2.
        drop(handle2);

        // That's okay - a spot was still freed up.
        let (handle2, buf2) = pool.get().await;
        assert!(buf2.is_empty());

        drop(handle1);
        drop(handle2);

        // Put an allocated buffer back.
        let (handle1, mut buf1) = pool.get().await;
        buf1.reserve(100);
        handle1.put(buf1);

        // Take the empty buffer.
        let (_handle2, _buf2) = pool.get().await;
        assert_eq!(buf2.capacity(), 0);

        // Sanity check pool has one item in it.
        assert_eq!(pool.rx.capacity(), Some(2));
        assert_eq!(pool.rx.len(), 1);

        // Run gc.
        pool.gc();

        // Pool has same amount of items after gc.
        assert_eq!(pool.rx.capacity(), Some(2));
        assert_eq!(pool.rx.len(), 1);

        // Our allocated buffer is now empty.
        let (_handle1, buf1) = pool.get().await;
        assert_eq!(buf1.capacity(), 0);

        Ok(())
    }
}
