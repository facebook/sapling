/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Representation of tree in EdenFS.
//!
//! Structs in this file should be keep in sync with `eden/fs/model/{Tree, TreeEntry}.h`.

use std::collections::HashMap;

use anyhow::Result;
use manifest::FileType;
use storemodel::FileAuxData;
use storemodel::TreeItemFlag;
use types::HgId;
use types::PathComponentBuf;

use crate::ffi::ffi::Tree;
use crate::ffi::ffi::TreeAuxData;
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
        hgid: HgId,
        flag: TreeItemFlag,
        aux: &HashMap<HgId, FileAuxData>,
    ) -> Option<Result<Self>> {
        let (ttype, hash, size, content_sha1, content_blake3) = match flag {
            TreeItemFlag::Directory => (TreeEntryType::Tree, hgid, None, None, None),
            TreeItemFlag::File(file_type) => {
                let entry_type = TreeEntryType::from_file_type(file_type)?;
                if let Some(aux_data) = aux.get(&hgid) {
                    (
                        entry_type,
                        hgid,
                        Some(aux_data.total_size),
                        Some(aux_data.sha1),
                        Some(aux_data.blake3),
                    )
                } else {
                    (entry_type, hgid, None, None, None)
                }
            }
        };

        let entry = TreeEntry {
            hash: hash.into_byte_array(),
            name: path.as_byte_slice().to_vec(),
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

impl TryFrom<Box<dyn storemodel::TreeEntry>> for Tree {
    type Error = anyhow::Error;

    fn try_from(value: Box<dyn storemodel::TreeEntry>) -> Result<Self, Self::Error> {
        let aux_map: HashMap<HgId, FileAuxData> =
            value.file_aux_iter()?.collect::<Result<HashMap<_, _>>>()?;
        let entries = value
            .iter()?
            .filter_map(|fallible| match fallible {
                Err(e) => Some(Err(e)),
                Ok((path, id, flag)) => TreeEntry::try_from_path_node(path, id, flag, &aux_map),
            })
            .collect::<Result<Vec<_>>>()?;
        let aux_data = match value.aux_data()? {
            Some(aux) => aux.into(),
            None => TreeAuxData {
                digest_size: 0,
                digest_hash: [0u8; 32],
            },
        };

        Ok(Tree { entries, aux_data })
    }
}
