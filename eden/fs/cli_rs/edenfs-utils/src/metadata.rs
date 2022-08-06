/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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

#[cfg(any(target_os = "macos", target_os = "linux"))]
use nix::sys::stat::Mode;

/// Metadata helper methods that map equivalent methods for the
/// edenfs purposes
pub trait MetadataExt {
    /// Returns the ID of the device containing the file
    fn eden_dev(&self) -> u64;

    /// Returns the file size
    fn eden_file_size(&self) -> u64;

    fn is_setuid_set(&self) -> bool;
}

#[cfg(windows)]
impl MetadataExt for Metadata {
    fn eden_dev(&self) -> u64 {
        // Dummy value
        0
    }

    fn eden_file_size(&self) -> u64 {
        self.file_size()
    }

    fn is_setuid_set(&self) -> bool {
        // This doesn't exist for windows
        false
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

    fn is_setuid_set(&self) -> bool {
        let isuid = Mode::S_ISUID;
        self.st_mode() & isuid.bits() != 0
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

    fn is_setuid_set(&self) -> bool {
        let isuid = Mode::S_ISUID;
        self.mode() & (isuid.bits() as u32) != 0
    }
}
