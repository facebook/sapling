/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::store::BlockId;

/// `TreeStateRoot` contains block id to the root `Tree`, and other metadata.
#[derive(Default)]
pub struct TreeStateRoot {
    version: u32,
    file_count: u32,
    tree_block_id: BlockId,
    metadata: Box<[u8]>,
    dirty: bool,
}

impl TreeStateRoot {
    pub fn new(version: u32, file_count: u32, tree_block_id: BlockId, metadata: Box<[u8]>) -> Self {
        TreeStateRoot {
            version,
            file_count,
            tree_block_id,
            metadata,
            dirty: false,
        }
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn set_version(&mut self, value: u32) {
        self.version = value;
        self.dirty = true;
    }

    pub fn file_count(&self) -> u32 {
        self.file_count
    }

    pub fn set_file_count(&mut self, value: u32) {
        self.file_count = value;
        self.dirty = true;
    }

    pub fn tree_block_id(&self) -> BlockId {
        self.tree_block_id
    }

    pub fn set_tree_block_id(&mut self, value: BlockId) {
        self.tree_block_id = value;
        self.dirty = true;
    }

    pub fn metadata(&self) -> &Box<[u8]> {
        &self.metadata
    }

    pub fn set_metadata(&mut self, value: Box<[u8]>) {
        self.metadata = value;
        self.dirty = true;
    }
}
