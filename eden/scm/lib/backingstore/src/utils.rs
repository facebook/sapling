/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use types::Key;
use types::Node;
use types::RepoPath;

pub fn key_from_path_node_slice(path: &[u8], node: &[u8]) -> Result<Key> {
    let path = RepoPath::from_utf8(path)?.to_owned();
    let node = Node::from_slice(node)?;
    Ok(Key::new(path, node))
}
