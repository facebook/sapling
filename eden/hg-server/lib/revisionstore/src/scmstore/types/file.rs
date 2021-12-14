/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;

use edenapi_types::{FileEntry as EdenApiFileEntry, FileError as EdenApiFileError};
use minibytes::Bytes;
use types::{Key, Parents};

use crate::{
    datastore::{strip_metadata, Metadata},
    indexedlogdatastore::Entry,
    redacted::is_redacted,
};

/// A strongly-typed file entry type. Like EdenApi's FileEntry type, but intended to support
/// Mercurial's local use cases rather than communication with EdenApi. Unlike EdenApi's FileEntry,
/// `RedactedFile` and `LfsPointer` are expressed as enum variants, rather than as errors when attempting
/// to read the file blob.
#[derive(Clone, Debug)]
pub struct StoreFile {
    key: Option<Key>,
    #[allow(dead_code)]
    parents: Option<Parents>,
    entry_metadata: Option<Metadata>,

    /// The meaning of the raw_content field depends on the StoreFileKind
    raw_content: Option<Bytes>,

    kind: StoreFileKind,
}

/// The different kinds of "file-like" entities you might come across in various file-oriented APIs
#[derive(Clone, Debug)]
enum StoreFileKind {
    // TODO(meyer): Do we need a separate "LfsFile" variant?
    /// A file. May be LFS or non-LFS, but its contents are immediately available without
    /// access to another store, unlike an LfsPointer.
    File {
        stripped_content: Option<Bytes>,
        #[allow(dead_code)]
        copied_from: Option<Key>,
    },

    // TODO(meyer): Parse out the LfsPointersEntry?
    /// An LFS Pointer. Contains the content-based hashes used to look up an LFS File.
    LfsPointer,

    /// A redacted file. The contents of a redacted file are no longer accessible, and instead are
    /// replaced with a special "tombstone" string.
    RedactedFile,
}

impl TryFrom<Entry> for StoreFile {
    type Error = Error;

    fn try_from(mut v: Entry) -> Result<Self, Self::Error> {
        let raw_content = v.content()?;
        let key = v.key().clone();
        let entry_metadata = v.metadata().clone();

        if is_redacted(&raw_content) {
            return Ok(StoreFile {
                key: Some(key),
                parents: None,
                raw_content: Some(raw_content),
                entry_metadata: Some(entry_metadata),
                kind: StoreFileKind::RedactedFile,
            });
        }

        // TODO(meyer): Delete when ExtStoredPolicy is removed.
        if entry_metadata.is_lfs() {
            return Ok(StoreFile {
                key: Some(key),
                parents: None,
                entry_metadata: Some(entry_metadata),
                raw_content: Some(raw_content),
                kind: StoreFileKind::LfsPointer,
            });
        }

        let (stripped, copied) = strip_metadata(&raw_content)?;
        Ok(StoreFile {
            key: Some(key),
            parents: None,
            entry_metadata: Some(entry_metadata),
            raw_content: Some(raw_content),
            kind: StoreFileKind::File {
                stripped_content: Some(stripped),
                copied_from: copied,
            },
        })
    }
}

impl TryFrom<EdenApiFileEntry> for StoreFile {
    type Error = Error;

    fn try_from(v: EdenApiFileEntry) -> Result<Self, Self::Error> {
        // TODO(meyer): Optimize this to remove unnecessary clones.
        use EdenApiFileError::*;
        v.data_checked().map_or_else(
            |e| match e {
                Corrupt(_) => Err(Error::from(e)),
                Redacted(key, raw_content) => Ok(StoreFile {
                    key: Some(key),
                    parents: Some(v.parents().clone()),
                    raw_content: Some(raw_content.into()),
                    entry_metadata: Some(v.metadata().clone()),
                    kind: StoreFileKind::RedactedFile,
                }),
                Lfs(key, raw_content) => Ok(StoreFile {
                    key: Some(key),
                    parents: Some(v.parents().clone()),
                    raw_content: Some(raw_content.into()),
                    entry_metadata: Some(v.metadata().clone()),
                    kind: StoreFileKind::LfsPointer,
                }),
            },
            |raw_content_checked| {
                let raw_content_checked = raw_content_checked.into();
                let (stripped, copied) = strip_metadata(&raw_content_checked)?;
                Ok(StoreFile {
                    key: Some(v.key().clone()),
                    parents: Some(v.parents().clone()),
                    entry_metadata: Some(v.metadata().clone()),
                    raw_content: Some(raw_content_checked),
                    kind: StoreFileKind::File {
                        stripped_content: Some(stripped),
                        copied_from: copied,
                    },
                })
            },
        )
    }
}

impl StoreFile {
    pub fn key(&self) -> Option<&Key> {
        self.key.as_ref()
    }

    /// The "logical" file content, as it will be written to a checkout, stripped of copy headers.
    ///
    /// Currently, this method will return the copy-header-stripped file contents for files,
    /// the redaction tombstone for "redacted" files, and None for LFS Pointers.
    pub fn content(&self) -> Option<&Bytes> {
        use StoreFileKind::*;
        match self.kind {
            File {
                stripped_content: ref c,
                ..
            } => c.as_ref(),
            RedactedFile => self.raw_content.as_ref(),
            // LFS pointers return None, they have no available content to be placed in the filesystem.
            _ => None,
        }
    }

    pub fn entry_metadata(&self) -> Option<&Metadata> {
        self.entry_metadata.as_ref()
    }
}
