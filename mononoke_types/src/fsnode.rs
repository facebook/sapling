// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure_ext::{bail_msg, chain::*};

use crate::blob::{Blob, BlobstoreValue, FsnodeBlob};
use crate::errors::*;
use crate::file_change::FileType;
use crate::hash::{Sha1, Sha256};
use crate::path::MPathElement;
use crate::thrift;
use crate::typed_hash::{ContentId, FsnodeId, FsnodeIdContext};

use rust_thrift::compact_protocol;
use std::collections::BTreeMap;

// An fsnode is a manifest node containing summary information about the
// files in the manifest that is useful in the implementation of
// filesystems.
//
// Fsnodes only exist for trees, and each fsnode is a structure that contains:
// * A list of its children, containing for each child:
//   - Name
//   - Type (regular file, executable file, symlink or sub-directory)
//   - The content id, size, and content hashes for files
//   - The fsnode id, simple format hashes, and summary counts for directories
// * The simple format hashes and summary counts for the directory itself
//
// Simple format hashes are a SHA-1 or SHA-256 hash of the directory's
// contents rendered in the following simple form: "HASH TYPE NAME\0"
// for each entry in the directory, where "TYPE" is one of 'file', 'exec',
// 'link', or 'tree', and HASH is the corresponding SHA-1 or SHA-256 hash
// of the entry (content for files, simple format for directories).
//
// The purpose of simple format hashes is to allow clients to construct
// their own hashes of data they have available locally, in order to do a
// whole-tree comparison with data stored in Mononoke.
//
// The summary counts stored for each directory are:
// * count of immediate child files
// * count of immediate child sub-directories
// * recursive count of descendant files
// * total size of immediate child files
// * rercursive total size of descendant files
//
// Unlike unodes, fsnodes are not repository-wide unique. If the same set of
// files and directories appear at different places in the commit graph,
// they will share fsnodes.

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Fsnode {
    subentries: BTreeMap<MPathElement, FsnodeEntry>,
    summary: FsnodeSummary,
}

impl Fsnode {
    pub fn new(subentries: BTreeMap<MPathElement, FsnodeEntry>, summary: FsnodeSummary) -> Self {
        Self {
            subentries,
            summary,
        }
    }

    pub fn get_fsnode_id(&self) -> FsnodeId {
        // TODO(mbthomas): try to avoid clone (although BonsaiChangeset and unodes have the same problems)
        *self.clone().into_blob().id()
    }

    pub fn lookup(&self, basename: &MPathElement) -> Option<&FsnodeEntry> {
        self.subentries.get(basename)
    }

    pub fn list(&self) -> impl Iterator<Item = (&MPathElement, &FsnodeEntry)> {
        self.subentries.iter()
    }

    pub fn into_subentries(self) -> BTreeMap<MPathElement, FsnodeEntry> {
        self.subentries
    }

    pub fn summary(&self) -> &FsnodeSummary {
        &self.summary
    }

    pub(crate) fn from_thrift(t: thrift::Fsnode) -> Result<Fsnode> {
        let subentries = t
            .subentries
            .into_iter()
            .map(|(basename, fsnode_entry)| {
                let basename = MPathElement::from_thrift(basename)?;
                let fsnode_entry = FsnodeEntry::from_thrift(fsnode_entry)?;
                Ok((basename, fsnode_entry))
            })
            .collect::<Result<_>>()?;
        let summary = FsnodeSummary::from_thrift(t.summary)?;
        Ok(Fsnode {
            subentries,
            summary,
        })
    }

    pub(crate) fn into_thrift(self) -> thrift::Fsnode {
        let subentries: BTreeMap<_, _> = self
            .subentries
            .into_iter()
            .map(|(basename, fsnode_entry)| (basename.into_thrift(), fsnode_entry.into_thrift()))
            .collect();
        let summary = self.summary.into_thrift();
        thrift::Fsnode {
            subentries,
            summary,
        }
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(bytes)
            .chain_err(ErrorKind::BlobDeserializeError("Fsnode".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum FsnodeEntry {
    File(FsnodeFile),
    Directory(FsnodeDirectory),
}

impl FsnodeEntry {
    pub(crate) fn from_thrift(t: thrift::FsnodeEntry) -> Result<FsnodeEntry> {
        match t {
            thrift::FsnodeEntry::File(fsnode_file) => {
                let fsnode_file = FsnodeFile::from_thrift(fsnode_file)?;
                Ok(FsnodeEntry::File(fsnode_file))
            }
            thrift::FsnodeEntry::Directory(fsnode_directory) => {
                let fsnode_directory = FsnodeDirectory::from_thrift(fsnode_directory)?;
                Ok(FsnodeEntry::Directory(fsnode_directory))
            }
            thrift::FsnodeEntry::UnknownField(unknown) => bail_msg!(
                "Unknown field encountered when parsing thrift::FsnodeEntry: {}",
                unknown,
            ),
        }
    }

    pub(crate) fn into_thrift(self) -> thrift::FsnodeEntry {
        match self {
            FsnodeEntry::File(fsnode_file) => thrift::FsnodeEntry::File(fsnode_file.into_thrift()),
            FsnodeEntry::Directory(fsnode_directory) => {
                thrift::FsnodeEntry::Directory(fsnode_directory.into_thrift())
            }
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct FsnodeFile {
    content_id: ContentId,
    file_type: FileType,
    size: u64,
    content_sha1: Sha1,
    content_sha256: Sha256,
}

impl FsnodeFile {
    pub fn new(
        content_id: ContentId,
        file_type: FileType,
        size: u64,
        content_sha1: Sha1,
        content_sha256: Sha256,
    ) -> Self {
        Self {
            content_id,
            file_type,
            size,
            content_sha1,
            content_sha256,
        }
    }

    pub fn content_id(&self) -> &ContentId {
        &self.content_id
    }

    pub fn file_type(&self) -> &FileType {
        &self.file_type
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn content_sha1(&self) -> &Sha1 {
        &self.content_sha1
    }

    pub fn content_sha256(&self) -> &Sha256 {
        &self.content_sha256
    }

    pub(crate) fn from_thrift(t: thrift::FsnodeFile) -> Result<FsnodeFile> {
        let content_id = ContentId::from_thrift(t.content_id)?;
        let file_type = FileType::from_thrift(t.file_type)?;
        let size = t.size as u64;
        let content_sha1 = Sha1::from_bytes(t.content_sha1.0)?;
        let content_sha256 = Sha256::from_bytes(t.content_sha256.0)?;
        Ok(FsnodeFile {
            content_id,
            file_type,
            size,
            content_sha1,
            content_sha256,
        })
    }

    pub(crate) fn into_thrift(self) -> thrift::FsnodeFile {
        thrift::FsnodeFile {
            content_id: self.content_id.into_thrift(),
            file_type: self.file_type.into_thrift(),
            size: self.size as i64,
            content_sha1: self.content_sha1.into_thrift(),
            content_sha256: self.content_sha256.into_thrift(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct FsnodeDirectory {
    id: FsnodeId,
    summary: FsnodeSummary,
}

impl FsnodeDirectory {
    pub fn new(id: FsnodeId, summary: FsnodeSummary) -> Self {
        Self { id, summary }
    }

    pub fn id(&self) -> &FsnodeId {
        &self.id
    }

    pub fn summary(&self) -> &FsnodeSummary {
        &self.summary
    }

    pub(crate) fn from_thrift(t: thrift::FsnodeDirectory) -> Result<FsnodeDirectory> {
        let id = FsnodeId::from_thrift(t.id)?;
        let summary = FsnodeSummary::from_thrift(t.summary)?;
        Ok(FsnodeDirectory { id, summary })
    }

    pub(crate) fn into_thrift(self) -> thrift::FsnodeDirectory {
        thrift::FsnodeDirectory {
            id: self.id.into_thrift(),
            summary: self.summary.into_thrift(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct FsnodeSummary {
    pub simple_format_sha1: Sha1,
    pub simple_format_sha256: Sha256,
    pub child_files_count: u64,
    pub child_files_total_size: u64,
    pub child_dirs_count: u64,
    pub descendant_files_count: u64,
    pub descendant_files_total_size: u64,
}

impl FsnodeSummary {
    pub(crate) fn from_thrift(t: thrift::FsnodeSummary) -> Result<FsnodeSummary> {
        let simple_format_sha1 = Sha1::from_bytes(t.simple_format_sha1.0)?;
        let simple_format_sha256 = Sha256::from_bytes(t.simple_format_sha256.0)?;
        let child_files_count = t.child_files_count as u64;
        let child_files_total_size = t.child_files_total_size as u64;
        let child_dirs_count = t.child_dirs_count as u64;
        let descendant_files_count = t.descendant_files_count as u64;
        let descendant_files_total_size = t.descendant_files_total_size as u64;
        Ok(FsnodeSummary {
            simple_format_sha1,
            simple_format_sha256,
            child_files_count,
            child_files_total_size,
            child_dirs_count,
            descendant_files_count,
            descendant_files_total_size,
        })
    }

    pub(crate) fn into_thrift(self) -> thrift::FsnodeSummary {
        thrift::FsnodeSummary {
            simple_format_sha1: self.simple_format_sha1.into_thrift(),
            simple_format_sha256: self.simple_format_sha256.into_thrift(),
            child_files_count: self.child_files_count as i64,
            child_files_total_size: self.child_files_total_size as i64,
            child_dirs_count: self.child_dirs_count as i64,
            descendant_files_count: self.descendant_files_count as i64,
            descendant_files_total_size: self.descendant_files_total_size as i64,
        }
    }
}

impl BlobstoreValue for Fsnode {
    type Key = FsnodeId;

    fn into_blob(self) -> FsnodeBlob {
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        let mut context = FsnodeIdContext::new();
        context.update(&data);
        let id = context.finish();
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data().as_ref())
    }
}
