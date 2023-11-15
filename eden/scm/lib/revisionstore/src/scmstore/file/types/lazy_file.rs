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
use hgstore::strip_hg_file_metadata;
use minibytes::Bytes;
use types::HgId;
use types::Key;

use crate::indexedlogdatastore::Entry;
use crate::lfs::rebuild_metadata;
use crate::lfs::LfsPointersEntry;
use crate::scmstore::file::FileAuxData;
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
        }
    }

    /// Compute's the aux data associated with this file from the content.
    pub(crate) fn aux_data(&mut self) -> Result<FileAuxData> {
        // TODO(meyer): Implement the rest of the aux data fields
        let aux_data = match self {
            LazyFile::Lfs(content, _) => FileAuxData::from_content(&content),
            LazyFile::EdenApi(entry) if entry.aux_data.is_some() => entry
                .aux_data()
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("Invalid EdenAPI entry in LazyFile. Aux data is empty")
                })?
                .into(),
            _ => {
                let content = self.file_content()?;
                FileAuxData::from_content(&content)
            }
        };
        Ok(aux_data)
    }

    /// The file content, as would be found in the working copy (stripped of copy header)
    pub(crate) fn file_content(&mut self) -> Result<Bytes> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref mut entry) => strip_hg_file_metadata(&entry.content()?)?.0,
            Lfs(ref blob, _) => blob.clone(),
            ContentStore(ref blob, _) => strip_hg_file_metadata(blob)?.0,
            // TODO(meyer): Convert EdenApi to use minibytes
            EdenApi(ref entry) => strip_hg_file_metadata(&entry.data()?.into())?.0,
        })
    }

    /// The file content, as would be found in the working copy, and also with copy info
    pub(crate) fn file_content_with_copy_info(&mut self) -> Result<(Bytes, Option<Key>)> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref mut entry) => strip_hg_file_metadata(&entry.content()?)?,
            Lfs(ref blob, ref ptr) => (blob.clone(), ptr.copy_from().clone()),
            ContentStore(ref blob, _) => strip_hg_file_metadata(blob)?,
            EdenApi(ref entry) => strip_hg_file_metadata(&entry.data()?.into())?,
        })
    }

    /// The file content, as would be encoded in the Mercurial blob (with copy header)
    pub(crate) fn hg_content(&self) -> Result<Bytes> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref entry) => entry.content()?,
            Lfs(ref blob, ref ptr) => rebuild_metadata(blob.clone(), ptr),
            ContentStore(ref blob, _) => blob.clone(),
            EdenApi(ref entry) => entry.data()?.into(),
        })
    }

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
        })
    }

    /// Convert the LazyFile to an indexedlog Entry, if it should ever be written to IndexedLog cache
    pub(crate) fn indexedlog_cache_entry(&self, key: Key) -> Result<Option<Entry>> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref entry) => Some(entry.clone().with_key(key)),
            EdenApi(ref entry) => Some(Entry::new(
                key,
                entry.data()?.into(),
                entry.metadata()?.clone(),
            )),
            // LFS Files should be written to LfsCache instead
            Lfs(_, _) => None,
            // ContentStore handles caching internally
            ContentStore(_, _) => None,
        })
    }
}

impl TryFrom<Entry> for LfsPointersEntry {
    type Error = Error;

    fn try_from(e: Entry) -> Result<Self, Self::Error> {
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
