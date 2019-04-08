// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::{
    key::Key,
    node::Node,
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

pub fn node(hex: &str) -> Node {
    if hex.len() > Node::hex_len() {
        panic!(format!("invalid length for hex node: {}", hex));
    }
    if hex == "0" {
        panic!(format!("node 0 is special, use Node::null_id() to build"));
    }
    let mut buffer = String::new();
    for _i in 0..Node::hex_len() - hex.len() {
        buffer.push('0');
    }
    buffer.push_str(hex);
    Node::from_str(&buffer).unwrap()
}

pub fn key(path: &str, hexnode: &str) -> Key {
    Key::new(path.as_bytes().to_vec(), node(hexnode))
}

/// The null node id is special and it's semantics vary. A null key contains a null node id.
pub fn null_key(path: &str) -> Key {
    Key::new(path.as_bytes().to_vec(), Node::null_id().clone())
}
