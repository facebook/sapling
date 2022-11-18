/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use types::Key;
use types::Node;
use types::RepoPathBuf;

pub fn key_from_path_node_slice(node: &[u8]) -> Key {
    let node = Node::from_slice(node).unwrap();
    Key::new(RepoPathBuf::new(), node)
}
