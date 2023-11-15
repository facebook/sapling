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

use crate::ffi::ffi::Tree;
use crate::ffi::ffi::TreeEntry;
use crate::ffi::ffi::TreeEntryType;

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

impl TreeEntry {
    fn try_from_path_node(
        path: PathComponentBuf,
        node: FsNodeMetadata,
        aux: &HashMap<HgId, FileAuxData>,
    ) -> Option<Result<Self>> {
        let (ttype, hash, size, content_sha1, content_blake3) = match node {
            FsNodeMetadata::Directory(Some(hgid)) => (TreeEntryType::Tree, hgid, None, None, None),
            FsNodeMetadata::File(metadata) => {
                let entry_type = match TreeEntryType::from_file_type(metadata.file_type) {
                    None => return None,
                    Some(entry_type) => entry_type,
                };
                if let Some(aux_data) = aux.get(&metadata.hgid) {
                    (
                        entry_type,
                        metadata.hgid,
                        Some(aux_data.total_size),
                        Some(aux_data.sha1),
                        aux_data.seeded_blake3,
                    )
                } else {
                    (entry_type, metadata.hgid, None, None, None)
                }
            }
            _ => return Some(Err(format_err!("received an ephemeral directory"))),
        };

        let entry = TreeEntry {
            hash: hash.into_byte_array(),
            name: path.as_ref().as_byte_slice().to_vec(),
            ttype,
            has_size: size.is_some(),
            size: size.map_or(0, |size| size),
            has_sha1: content_sha1.is_some(),
            content_sha1: content_sha1
                .map_or([0u8; 20], |content_sha1| content_sha1.into_byte_array()),
            has_blake3: content_blake3.is_some(),
            content_blake3: content_blake3
                .map_or([0u8; 32], |content_blake3| content_blake3.into_byte_array()),
        };
        Some(Ok(entry))
    }
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

                Ok(Tree { entries })
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn sapling_tree_free(tree: *mut Tree) {
    assert!(!tree.is_null());
    let tree = unsafe { Box::from_raw(tree) };
    drop(tree);
}
