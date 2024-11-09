/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use edenapi_types::FileEntry;
use format_util::split_file_metadata;
use minibytes::Bytes;
use storemodel::SerializationFormat;
use types::HgId;
use types::Key;

use crate::indexedlogdatastore::Entry;
use crate::lfs::content_header_from_pointer;
use crate::lfs::rebuild_metadata;
use crate::lfs::LfsPointersEntry;
use crate::scmstore::file::FileAuxData;
use crate::Metadata;

/// A minimal file enum that simply wraps the possible underlying file types,
/// with no processing (so Entry might have the wrong Key.path, etc.)
#[derive(Debug)]
pub(crate) enum LazyFile {
    /// An entry from a local IndexedLog. The contained Key's path might not match the requested Key's path.
    IndexedLog(Entry, SerializationFormat),

    /// A local LfsStore entry.
    Lfs(Bytes, LfsPointersEntry, SerializationFormat),

    /// An SaplingRemoteApi FileEntry.
    SaplingRemoteApi(FileEntry, SerializationFormat),

    /// File content read from CAS (no hg header).
    Cas(Bytes),
}

impl LazyFile {
    #[allow(dead_code)]
    fn hgid(&self) -> Option<HgId> {
        use LazyFile::*;
        match self {
            IndexedLog(ref entry, _) => Some(entry.key().hgid),
            Lfs(_, ref ptr, _) => Some(ptr.hgid()),
            SaplingRemoteApi(ref entry, _) => Some(entry.key().hgid),
            Cas(_) => None,
        }
    }

    /// Compute's the aux data associated with this file from the content.
    pub(crate) fn aux_data(&self) -> Result<FileAuxData> {
        match self {
            LazyFile::SaplingRemoteApi(entry, _) if entry.aux_data.is_some() => {
                entry.aux_data().cloned().ok_or_else(|| {
                    anyhow::anyhow!("Invalid SaplingRemoteAPI entry in LazyFile. Aux data is empty")
                })
            }
            _ => {
                let (content, header) = self.file_content()?;
                let mut aux_data = FileAuxData::from_content(&content);

                // Content header (i.e. hg copy info) is not in the (pure) content. If we
                // have header in-hand, also include it in the aux data.
                aux_data.file_header_metadata = header;

                Ok(aux_data)
            }
        }
    }

    /// The file content, as would be found in the working copy (stripped of copy header), and the content header.
    /// Content header is `None` iff not available. If available but not set, content header is `Some(b"")`.
    pub(crate) fn file_content(&self) -> Result<(Bytes, Option<Bytes>)> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(entry, format) => split_file_metadata(&entry.content()?, *format),
            Lfs(blob, ptr, format) => {
                let content_header = match format {
                    SerializationFormat::Hg => Some(content_header_from_pointer(ptr)),
                    SerializationFormat::Git => None,
                };
                (blob.clone(), content_header)
            }
            SaplingRemoteApi(ref entry, format) => split_file_metadata(&entry.data()?, *format),
            Cas(data) => (data.clone(), None),
        })
    }

    /// The file content, as would be encoded in the Mercurial blob (with copy header)
    pub(crate) fn hg_content(&self) -> Result<Bytes> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref entry, _) => entry.content()?,
            Lfs(ref blob, ref ptr, _) => rebuild_metadata(blob.clone(), ptr),
            SaplingRemoteApi(ref entry, _) => entry.data()?,
            Cas(_) => bail!("CAS data has no copy info"),
        })
    }

    pub(crate) fn metadata(&self) -> Result<Metadata> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref entry, _) => entry.metadata().clone(),
            Lfs(_, ref ptr, _) => Metadata {
                size: Some(ptr.size()),
                flags: None,
            },
            SaplingRemoteApi(ref entry, _) => entry.metadata()?.clone(),
            Cas(data) => Metadata {
                size: Some(data.len() as u64),
                flags: None,
            },
        })
    }

    /// Convert the LazyFile to an indexedlog Entry, if it should ever be written to IndexedLog cache
    pub(crate) fn indexedlog_cache_entry(&self, key: Key) -> Result<Option<Entry>> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref entry, _) => Some(entry.clone().with_key(key)),
            SaplingRemoteApi(ref entry, _) => {
                Some(Entry::new(key, entry.data()?, entry.metadata()?.clone()))
            }
            // LFS Files should be written to LfsCache instead
            Lfs(_, _, _) => None,
            Cas(_) => None,
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
            bail!("failed to convert SaplingRemoteApi FileEntry to LFS pointer, is_lfs is false")
        }
    }
}
