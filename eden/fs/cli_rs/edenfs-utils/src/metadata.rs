/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::Metadata;

#[cfg(target_os = "linux")]
use std::os::linux::fs::MetadataExt as MetadataLinuxExt;
#[cfg(target_os = "macos")]
use std::os::unix::fs::MetadataExt as MetadataMacosExt;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt as MetadataWindowsExt;

/// Metadata helper methods that map equivalent methods for the
/// edenfs purposes
pub trait MetadataExt {
    /// Returns the ID of the device containing the file
    fn eden_dev(&self) -> u64;

    /// Returns the file size
    fn eden_file_size(&self) -> u64;
}

#[cfg(windows)]
impl MetadataExt for Metadata {
    fn eden_dev(&self) -> u64 {
        0
    }

    fn eden_file_size(&self) -> u64 {
        self.file_size()
    }
}

#[cfg(target_os = "linux")]
impl MetadataExt for Metadata {
    fn eden_dev(&self) -> u64 {
        self.st_dev()
    }

    fn eden_file_size(&self) -> u64 {
        // Use st_blocks as this represents the actual amount of
        // disk space allocated by the file, not its apparent
        // size.
        self.st_blocks() * 512
    }
}

#[cfg(target_os = "macos")]
impl MetadataExt for Metadata {
    fn eden_dev(&self) -> u64 {
        self.dev()
    }

    fn eden_file_size(&self) -> u64 {
        self.blocks() * 512
    }
}
