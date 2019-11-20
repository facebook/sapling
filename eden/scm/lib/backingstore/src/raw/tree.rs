/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Representation of tree in EdenFS.
//!
//! Structs in this file should be keep in sync with `eden/fs/model/{Tree, TreeEntry}.h`.

use crate::raw::CBytes;
use failure::{format_err, Fallible};
use manifest::{tree::List, FileType, FsNode};
use std::convert::TryFrom;
use types::PathComponentBuf;

#[repr(u8)]
pub enum TreeEntryType {
    Tree,
    RegularFile,
    ExecutableFile,
    Symlink,
}

impl From<FileType> for TreeEntryType {
    fn from(file_type: FileType) -> Self {
        match file_type {
            FileType::Regular => TreeEntryType::RegularFile,
            FileType::Executable => TreeEntryType::ExecutableFile,
            FileType::Symlink => TreeEntryType::Symlink,
        }
    }
}

#[repr(C)]
pub struct TreeEntry {
    hash: CBytes,
    name: CBytes,
    ttype: TreeEntryType,
    // Using pointer as `Option<T>`
    size: *mut u64,
    content_sha1: *mut CBytes,
}

impl TreeEntry {
    fn try_from_path_node(path: PathComponentBuf, node: FsNode) -> Fallible<Self> {
        let (ttype, hash) = match node {
            FsNode::Directory(Some(hgid)) => (TreeEntryType::Tree, hgid.as_ref().to_vec()),
            FsNode::File(metadata) => (metadata.file_type.into(), metadata.hgid.as_ref().to_vec()),
            _ => return Err(format_err!("received an ephemeral directory")),
        };

        Ok(TreeEntry {
            hash: hash.into(),
            name: path.as_ref().as_byte_slice().to_vec().into(),
            ttype,
            // TODO: we currently do not have these information stored in Mercurial.
            size: std::ptr::null_mut(),
            content_sha1: std::ptr::null_mut(),
        })
    }
}

#[repr(C)]
pub struct Tree {
    entries: *const TreeEntry,
    /// This makes sure `entries` above is pointing to a valid memory.
    entries_ptr: *mut Vec<TreeEntry>,
    length: usize,
    hash: CBytes,
}

impl TryFrom<List> for Tree {
    type Error = failure::Error;

    fn try_from(list: List) -> Result<Self, Self::Error> {
        match list {
            List::NotFound | List::File => Err(format_err!("not found")),
            List::Directory(list) => {
                let entries = list
                    .into_iter()
                    .map(|(path, node)| TreeEntry::try_from_path_node(path, node))
                    .collect::<Fallible<Vec<_>>>()?;

                let entries = Box::new(entries);
                let length = entries.len();

                Ok(Tree {
                    entries: entries.as_ptr(),
                    entries_ptr: Box::into_raw(entries),
                    length,
                    hash: Vec::new().into(),
                })
            }
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let entry = unsafe { Box::from_raw(self.entries_ptr) };
        drop(entry);
    }
}
