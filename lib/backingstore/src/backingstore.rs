// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use configparser::config::ConfigSet;
use configparser::hg::ConfigSetHgExt;
use failure::Fallible;
use revisionstore::{ContentStore, DataStore};
use std::path::Path;
use types::{Key, Node, RepoPath};

pub struct BackingStore {
    store: ContentStore,
}

impl BackingStore {
    pub fn new<P: AsRef<Path>>(repository: P) -> Fallible<Self> {
        let hg = repository.as_ref().join(".hg");
        let mut config = ConfigSet::new();
        config.load_system();
        config.load_user();
        config.load_hgrc(hg.join("hgrc"), "repository");

        let store = ContentStore::new(hg.join("store"), &config)?;

        Ok(Self { store })
    }

    pub fn get_blob(&self, path: &[u8], node: &[u8]) -> Fallible<Option<Vec<u8>>> {
        let path = RepoPath::from_utf8(path)?.to_owned();
        let node = Node::from_slice(node)?;
        let key = Key::new(path, node);

        // Return None for LFS blobs
        // TODO: LFS support
        if let Ok(Some(metadata)) = self.store.get_meta(&key) {
            if let Some(flag) = metadata.flags {
                if flag == 0x2000 {
                    return Ok(None);
                }
            }
        }

        self.store
            .get(&key)
            .map(|blob| blob.map(discard_metadata_header))
    }
}

/// Removes the possible metadata header at the beginning of a blob.
///
/// The metadata header is defined as the block surrounded by '\x01\x0A' at the beginning of the
/// blob. If there is no closing tag found in the blob, this function will simply return the
/// original blob.
///
/// See `edenscm/mercurial/filelog.py` for the Python implementation.
fn discard_metadata_header(data: Vec<u8>) -> Vec<u8> {
    // Returns when the blob less than 2 bytes long or no metadata header starting tag at the
    // beginning
    if data.len() < 2 || !(data[0] == 0x01 && data[1] == 0x0A) {
        return data;
    }

    // Finds the position of the closing tag
    let closing_tag = data.windows(2).skip(2).position(|bytes| match *bytes {
        [a, b] => a == 0x01 && b == 0x0A,
        // Rust cannot infer that `.windows` only gives us a two-element slice so we have to write
        // the predicate this way to provide the default clause.
        _ => false,
    });

    if let Some(idx) = closing_tag {
        // Skip two bytes for the starting tag and two bytes for the closing tag
        data.into_iter().skip(2 + idx + 2).collect()
    } else {
        data
    }
}

#[test]
fn test_discard_metadata_header() {
    assert_eq!(discard_metadata_header(vec![]), vec![]);
    assert_eq!(discard_metadata_header(vec![0x1]), vec![0x1]);
    assert_eq!(discard_metadata_header(vec![0x1, 0x1]), vec![0x1, 0x1]);
    assert_eq!(discard_metadata_header(vec![0x1, 0xA]), vec![0x1, 0xA]);

    // Empty metadata header and empty blob
    assert_eq!(discard_metadata_header(vec![0x1, 0xA, 0x1, 0xA]), vec![]);
    // Metadata header with some data but empty blob
    assert_eq!(
        discard_metadata_header(vec![0x1, 0xA, 0xA, 0xB, 0xC, 0x1, 0xA]),
        vec![]
    );
    // Metadata header with data and blob
    assert_eq!(
        discard_metadata_header(vec![0x1, 0xA, 0xA, 0xB, 0xC, 0x1, 0xA, 0xA, 0xB, 0xC]),
        vec![0xA, 0xB, 0xC]
    );
}
