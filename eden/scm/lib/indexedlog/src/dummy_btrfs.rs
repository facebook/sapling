/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io;

#[derive(Clone, Copy)]
pub(crate) struct Metadata {}

impl Metadata {
    pub fn size(&self) -> u64 {
        0
    }
}

pub(crate) fn physical_size(_file: &File, _since: Option<Metadata>) -> io::Result<Metadata> {
    Err(std::io::Error::other("btrfs not supported"))
}

pub(crate) fn set_property(_file: &File, _name: &str, _value: &str) -> io::Result<()> {
    Err(std::io::Error::other("btrfs not supported"))
}
