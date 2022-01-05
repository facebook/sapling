/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::slice;

use anyhow::Result;
use types::Key;

use crate::utils::key_from_path_node_slice;

// Number of bytes of a node.
const NODE_LENGTH: usize = 20;

#[repr(C)]
pub struct Request {
    path: *const u8,
    length: usize,
    node: *const u8,
}

impl Request {
    pub fn try_into_key(&self) -> Result<Key> {
        let path: &[u8] = unsafe { slice::from_raw_parts(self.path, self.length) };
        let node: &[u8] = unsafe { slice::from_raw_parts(self.node, NODE_LENGTH) };
        key_from_path_node_slice(path, node)
    }
}
