/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod packer;

#[cfg(test)]
mod quickcheck_types;

#[derive(Debug, Clone)]
pub struct MetadataEntry {
    key: String,
    value: String,
}

impl MetadataEntry {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}
