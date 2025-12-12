/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::slice;

use types::Key;
use types::Node;
use types::RepoPathBuf;

use crate::ffi::ffi::Request;

// Number of bytes of a node.
const NODE_LENGTH: usize = 20;

impl Request<'_> {
    pub fn key(&self) -> Key {
        if self.oid.is_empty() {
            Key::default()
        } else {
            let node: &[u8] =
                unsafe { slice::from_raw_parts(&self.oid[1] as *const u8, NODE_LENGTH) };
            Key::new(RepoPathBuf::new(), Node::from_slice(node).unwrap())
        }
    }
}
