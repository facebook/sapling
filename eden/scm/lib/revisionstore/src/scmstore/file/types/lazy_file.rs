/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use edenapi_types::FileEntry;
use minibytes::Bytes;
use tracing::instrument;
use types::HgId;
use types::Key;

use crate::datastore::strip_metadata;
use crate::indexedlogdatastore::Entry;
use crate::lfs::rebuild_metadata;
use crate::lfs::LfsPointersEntry;
use crate::memcache::McData;
use crate::scmstore::file::FileAuxData;
use crate::ContentHash;
use crate::Metadata;

/// A minimal file enum that simply wraps the possible underlying file types,
/// with no processing (so Entry might have the wrong Key.path, etc.)
#[derive(Debug)]
pub(crate) enum LazyFile {
    /// A response from calling into the legacy storage API
    ContentStore(Bytes, Metadata),

    /// An entry from a local IndexedLog. The contained Key's path might not match the requested Key's path.
    IndexedLog(Entry),

    /// A local LfsStore entry.
    Lfs(Bytes, LfsPointersEntry),

    /// An EdenApi FileEntry.
    EdenApi(FileEntry),

    /// A memcache entry, convertable to Entry. In this case the Key's path should match the requested Key's path.
    Memcache(McData),
}

impl LazyFile {
    #[allow(dead_code)]
    fn hgid(&self) -> Option<HgId> {
        use LazyFile::*;
        match self {
            ContentStore(_, _) => None,
            IndexedLog(ref entry) => Some(entry.key().hgid),
            Lfs(_, ref ptr) => Some(ptr.hgid()),
            EdenApi(ref entry) => Some(entry.key().hgid),
            Memcache(ref entry) => Some(entry.key.hgid),
        }
    }

    /// Compute's the aux data associated with this file from the content.
    #[instrument(level = "debug", skip(self))]
    pub(crate) fn aux_data(&mut self) -> Result<FileAuxData> {
        // TODO(meyer): Implement the rest of the aux data fields
        Ok(if let LazyFile::Lfs(content, ref ptr) = self {
            FileAuxData {
                total_size: content.len() as u64,
                content_id: ContentHash::content_id(&content),
                content_sha1: ContentHash::sha1(&content),
                content_sha256: ptr.sha256(),
            }
        } else {
            let content = self.file_content()?;
            FileAuxData {
                total_size: content.len() as u64,
                content_id: ContentHash::content_id(&content),
                content_sha1: ContentHash::sha1(&content),
                content_sha256: ContentHash::sha256(&content).unwrap_sha256(),
            }
        })
    }

    /// The file content, as would be found in the working copy (stripped of copy header)
    #[instrument(level = "debug", skip(self))]
    pub(crate) fn file_content(&mut self) -> Result<Bytes> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref mut entry) => strip_metadata(&entry.content()?)?.0,
            Lfs(ref blob, _) => blob.clone(),
            ContentStore(ref blob, _) => strip_metadata(blob)?.0,
            // TODO(meyer): Convert EdenApi to use minibytes
            EdenApi(ref entry) => strip_metadata(&entry.data()?.into())?.0,
            Memcache(ref entry) => strip_metadata(&entry.data)?.0,
        })
    }

    /// The file content, as would be encoded in the Mercurial blob (with copy header)
    #[instrument(level = "debug", skip(self))]
    pub(crate) fn hg_content(&mut self) -> Result<Bytes> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref mut entry) => entry.content()?,
            Lfs(ref blob, ref ptr) => rebuild_metadata(blob.clone(), ptr),
            ContentStore(ref blob, _) => blob.clone(),
            EdenApi(ref entry) => entry.data()?.into(),
            Memcache(ref entry) => entry.data.clone(),
        })
    }

    #[instrument(level = "debug", skip(self))]
    pub(crate) fn metadata(&self) -> Result<Metadata> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref entry) => entry.metadata().clone(),
            Lfs(_, ref ptr) => Metadata {
                size: Some(ptr.size()),
                flags: None,
            },
            ContentStore(_, ref meta) => meta.clone(),
            EdenApi(ref entry) => entry.metadata()?.clone(),
            Memcache(ref entry) => entry.metadata.clone(),
        })
    }

    /// Convert the LazyFile to an indexedlog Entry, if it should ever be written to IndexedLog cache
    #[instrument(level = "debug", skip(self))]
    pub(crate) fn indexedlog_cache_entry(&self, key: Key) -> Result<Option<Entry>> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref entry) => Some(entry.clone().with_key(key)),
            EdenApi(ref entry) => Some(Entry::new(
                key,
                entry.data()?.into(),
                entry.metadata()?.clone(),
            )),
            // TODO(meyer): We shouldn't ever need to replace the key with Memcache, can probably just clone this.
            Memcache(ref entry) => Some({
                let entry: Entry = entry.clone().into();
                entry.with_key(key)
            }),
            // LFS Files should be written to LfsCache instead
            Lfs(_, _) => None,
            // ContentStore handles caching internally
            ContentStore(_, _) => None,
        })
    }
}

impl TryFrom<McData> for LfsPointersEntry {
    type Error = Error;

    fn try_from(e: McData) -> Result<Self, Self::Error> {
        if e.metadata.is_lfs() {
            Ok(LfsPointersEntry::from_bytes(e.data, e.key.hgid)?)
        } else {
            bail!("failed to convert McData entry to LFS pointer, is_lfs is false")
        }
    }
}

impl TryFrom<Entry> for LfsPointersEntry {
    type Error = Error;

    fn try_from(mut e: Entry) -> Result<Self, Self::Error> {
        if e.metadata().is_lfs() {
            Ok(LfsPointersEntry::from_bytes(e.content()?, e.key().hgid)?)
        } else {
            bail!("failed to convert entry to LFS pointer, is_lfs is false")
        }
    }
}

impl TryFrom<FileEntry> for LfsPointersEntry {
    type Error = Error;

    fn try_from(e: FileEntry) -> Result<Self, Self::Error> {
        if e.metadata()?.is_lfs() {
            Ok(LfsPointersEntry::from_bytes(e.data()?, e.key().hgid)?)
        } else {
            bail!("failed to convert EdenApi FileEntry to LFS pointer, is_lfs is false")
        }
    }
}
