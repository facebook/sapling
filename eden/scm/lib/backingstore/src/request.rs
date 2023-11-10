/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::slice;

use types::Key;
use types::Node;
use types::RepoPathBuf;

// Number of bytes of a node.
const NODE_LENGTH: usize = 20;

#[repr(C)]
pub struct Request {
    node: *const u8,
}

impl Request {
    pub fn key(&self) -> Key {
        let node: &[u8] = unsafe { slice::from_raw_parts(self.node, NODE_LENGTH) };
        Key::new(RepoPathBuf::new(), Node::from_slice(node).unwrap())
    }
}
