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
    /// An entry from a local IndexedLog. The contained Key's path might not match the requested Key's path.
    IndexedLog(Entry),

    /// A local LfsStore entry.
    Lfs(Bytes, LfsPointersEntry),

    /// An SaplingRemoteApi FileEntry.
    SaplingRemoteApi(FileEntry),

    /// File content read from CAS (no hg header).
    Cas(Bytes),
}

impl LazyFile {
    #[allow(dead_code)]
    fn hgid(&self) -> Option<HgId> {
        use LazyFile::*;
        match self {
            IndexedLog(ref entry) => Some(entry.key().hgid),
            Lfs(_, ref ptr) => Some(ptr.hgid()),
            SaplingRemoteApi(ref entry) => Some(entry.key().hgid),
            Cas(_) => None,
        }
    }

    /// Compute's the aux data associated with this file from the content.
    pub(crate) fn aux_data(&mut self) -> Result<FileAuxData> {
        let aux_data = match self {
            LazyFile::Lfs(content, _) => FileAuxData::from_content(content),
            LazyFile::SaplingRemoteApi(entry) if entry.aux_data.is_some() => {
                entry.aux_data().cloned().ok_or_else(|| {
                    anyhow::anyhow!("Invalid SaplingRemoteAPI entry in LazyFile. Aux data is empty")
                })?
            }
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
            // TODO(meyer): Convert SaplingRemoteApi to use minibytes
            SaplingRemoteApi(ref entry) => strip_hg_file_metadata(&entry.data()?.into())?.0,
            Cas(data) => data.clone(),
        })
    }

    /// The file content, as would be found in the working copy, and also with copy info
    pub(crate) fn file_content_with_copy_info(&mut self) -> Result<(Bytes, Option<Key>)> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref mut entry) => strip_hg_file_metadata(&entry.content()?)?,
            Lfs(ref blob, ref ptr) => (blob.clone(), ptr.copy_from().clone()),
            SaplingRemoteApi(ref entry) => strip_hg_file_metadata(&entry.data()?.into())?,
            Cas(_) => bail!("CAS data has no copy info"),
        })
    }

    /// The file content, as would be encoded in the Mercurial blob (with copy header)
    pub(crate) fn hg_content(&self) -> Result<Bytes> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref entry) => entry.content()?,
            Lfs(ref blob, ref ptr) => rebuild_metadata(blob.clone(), ptr),
            SaplingRemoteApi(ref entry) => entry.data()?.into(),
            Cas(_) => bail!("CAS data has no copy info"),
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
            SaplingRemoteApi(ref entry) => entry.metadata()?.clone(),
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
            IndexedLog(ref entry) => Some(entry.clone().with_key(key)),
            SaplingRemoteApi(ref entry) => Some(Entry::new(
                key,
                entry.data()?.into(),
                entry.metadata()?.clone(),
            )),
            // LFS Files should be written to LfsCache instead
            Lfs(_, _) => None,
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
