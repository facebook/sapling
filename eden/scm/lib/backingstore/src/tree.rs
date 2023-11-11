/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Representation of tree in EdenFS.
//!
//! Structs in this file should be keep in sync with `eden/fs/model/{Tree, TreeEntry}.h`.

use std::collections::HashMap;

use anyhow::format_err;
use anyhow::Result;
use manifest::FileType;
use manifest::FsNodeMetadata;
use manifest::List;
use revisionstore::scmstore::file::FileAuxData;
use types::HgId;
use types::PathComponentBuf;

use crate::cbytes::CBytes;

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
    content_blake3: *mut CBytes,
}

impl TreeEntry {
    fn try_from_path_node(
        path: PathComponentBuf,
        node: FsNodeMetadata,
        aux: &HashMap<HgId, FileAuxData>,
    ) -> Option<Result<Self>> {
        let (ttype, hash, size, content_sha1, content_blake3) = match node {
            FsNodeMetadata::Directory(Some(hgid)) => (
                TreeEntryType::Tree,
                hgid.as_ref().to_vec(),
                None,
                None,
                None,
            ),
            FsNodeMetadata::File(metadata) => {
                let entry_type = match TreeEntryType::from_file_type(metadata.file_type) {
                    None => return None,
                    Some(entry_type) => entry_type,
                };
                if let Some(aux_data) = aux.get(&metadata.hgid) {
                    (
                        entry_type,
                        metadata.hgid.as_ref().to_vec(),
                        Some(aux_data.total_size),
                        Some(aux_data.sha1),
                        aux_data.seeded_blake3,
                    )
                } else {
                    (
                        entry_type,
                        metadata.hgid.as_ref().to_vec(),
                        None,
                        None,
                        None,
                    )
                }
            }
            _ => return Some(Err(format_err!("received an ephemeral directory"))),
        };

        let entry = TreeEntry {
            hash: hash.into(),
            name: path.as_ref().as_byte_slice().to_vec().into(),
            ttype,
            size: size.map_or(std::ptr::null_mut(), |size| {
                let boxed_size = Box::new(size);
                Box::into_raw(boxed_size)
            }),
            content_sha1: content_sha1.map_or(std::ptr::null_mut(), |content_sha1| {
                let boxed_sha1 = Box::new(content_sha1.as_ref().to_vec().into());
                Box::into_raw(boxed_sha1)
            }),
            content_blake3: content_blake3.map_or(std::ptr::null_mut(), |content_blake3| {
                let boxed_blake3 = Box::new(content_blake3.as_ref().to_vec().into());
                Box::into_raw(boxed_blake3)
            }),
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
}

impl TryFrom<(List, HashMap<HgId, FileAuxData>)> for Tree {
    type Error = anyhow::Error;

    fn try_from(entries: (List, HashMap<HgId, FileAuxData>)) -> Result<Self, Self::Error> {
        match entries.0 {
            List::NotFound | List::File => Err(format_err!("not found")),
            List::Directory(list) => {
                let entries = list
                    .into_iter()
                    .filter_map(|(path, node)| {
                        TreeEntry::try_from_path_node(path, node, &entries.1)
                    })
                    .collect::<Result<Vec<_>>>()?;

                let entries = Box::new(entries);
                let length = entries.len();

                Ok(Tree {
                    entries: entries.as_ptr(),
                    entries_ptr: Box::into_raw(entries),
                    length,
                })
            }
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let entries = unsafe { Box::from_raw(self.entries_ptr) };
        for entry in entries.iter() {
            let size = unsafe {
                if entry.size.is_null() {
                    None
                } else {
                    Some(Box::from_raw(entry.size))
                }
            };
            drop(size);
            let content_sha1 = unsafe {
                if entry.content_sha1.is_null() {
                    None
                } else {
                    Some(Box::from_raw(entry.content_sha1))
                }
            };
            drop(content_sha1);
            let content_blake3 = unsafe {
                if entry.content_blake3.is_null() {
                    None
                } else {
                    Some(Box::from_raw(entry.content_blake3))
                }
            };
            drop(content_blake3);
        }
        drop(entries);
    }
}

#[no_mangle]
pub extern "C" fn sapling_tree_free(tree: *mut Tree) {
    assert!(!tree.is_null());
    let tree = unsafe { Box::from_raw(tree) };
    drop(tree);
}
