/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Representation of tree in EdenFS.
//!
//! Structs in this file should be keep in sync with `eden/fs/model/{Tree, TreeEntry}.h`.

use anyhow::format_err;
use anyhow::Result;
use manifest::FileType;
use manifest::FsNodeMetadata;
use manifest::List;
use types::PathComponentBuf;

use crate::raw::CBytes;

#[repr(u8)]
pub enum TreeEntryType {
    Tree,
    RegularFile,
    ExecutableFile,
    Symlink,
}

impl TreeEntryType {
    /// Returns `None` for entries that need to be skipped.
    fn from_file_type(file_type: FileType) -> Option<Self> {
        let entry_type = match file_type {
            FileType::Regular => TreeEntryType::RegularFile,
            FileType::Executable => TreeEntryType::ExecutableFile,
            FileType::Symlink => TreeEntryType::Symlink,
            FileType::GitSubmodule => return None,
        };
        Some(entry_type)
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
    fn try_from_path_node(path: PathComponentBuf, node: FsNodeMetadata) -> Option<Result<Self>> {
        let (ttype, hash) = match node {
            FsNodeMetadata::Directory(Some(hgid)) => (TreeEntryType::Tree, hgid.as_ref().to_vec()),
            FsNodeMetadata::File(metadata) => {
                let entry_type = match TreeEntryType::from_file_type(metadata.file_type) {
                    None => return None,
                    Some(entry_type) => entry_type,
                };
                (entry_type, metadata.hgid.as_ref().to_vec())
            }
            _ => return Some(Err(format_err!("received an ephemeral directory"))),
        };

        let entry = TreeEntry {
            hash: hash.into(),
            name: path.as_ref().as_byte_slice().to_vec().into(),
            ttype,
            // TODO: we currently do not have these information stored in Mercurial.
            size: std::ptr::null_mut(),
            content_sha1: std::ptr::null_mut(),
        };
        Some(Ok(entry))
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
    type Error = anyhow::Error;

    fn try_from(list: List) -> Result<Self, Self::Error> {
        match list {
            List::NotFound | List::File => Err(format_err!("not found")),
            List::Directory(list) => {
                let entries = list
                    .into_iter()
                    .filter_map(|(path, node)| TreeEntry::try_from_path_node(path, node))
                    .collect::<Result<Vec<_>>>()?;

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

#[no_mangle]
pub extern "C" fn sapling_tree_free(tree: *mut Tree) {
    assert!(!tree.is_null());
    let tree = unsafe { Box::from_raw(tree) };
    drop(tree);
}
