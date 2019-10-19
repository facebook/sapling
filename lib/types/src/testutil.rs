// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::{
    dataentry::DataEntry,
    hgid::HgId,
    key::Key,
    parents::Parents,
    path::{PathComponent, PathComponentBuf, RepoPath, RepoPathBuf},
};

pub fn repo_path(s: &str) -> &RepoPath {
    if s == "" {
        panic!(format!(
            "the empty repo path is special, use RepoPath::empty() to build"
        ));
    }
    RepoPath::from_str(s).unwrap()
}

pub fn repo_path_buf(s: &str) -> RepoPathBuf {
    if s == "" {
        panic!(format!(
            "the empty repo path is special, use RepoPathBuf::new() to build"
        ));
    }
    RepoPathBuf::from_string(s.to_owned()).unwrap()
}

pub fn path_component(s: &str) -> &PathComponent {
    PathComponent::from_str(s).unwrap()
}

pub fn path_component_buf(s: &str) -> PathComponentBuf {
    PathComponentBuf::from_string(s.to_owned()).unwrap()
}

pub fn hgid(hex: &str) -> HgId {
    if hex.len() > HgId::hex_len() {
        panic!(format!("invalid length for hex hgid: {}", hex));
    }
    if hex == "0" {
        panic!(format!("hgid 0 is special, use HgId::null_id() to build"));
    }
    let mut buffer = String::new();
    for _i in 0..HgId::hex_len() - hex.len() {
        buffer.push('0');
    }
    buffer.push_str(hex);
    HgId::from_str(&buffer).unwrap()
}

pub fn key(path: &str, hexnode: &str) -> Key {
    Key::new(repo_path_buf(path), hgid(hexnode))
}

/// The null hgid id is special and it's semantics vary. A null key contains a null hgid id.
pub fn null_key(path: &str) -> Key {
    Key::new(repo_path_buf(path), HgId::null_id().clone())
}

pub fn data_entry(key: Key, data: impl AsRef<[u8]>) -> DataEntry {
    DataEntry::new(key, data.as_ref().into(), Parents::None)
}
