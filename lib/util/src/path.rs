// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Path-related utilities.

/// Normalize a normalized Path for display.
///
/// This removes the UNC prefix `\\?\` on Windows.
pub fn normalize_for_display(path: &str) -> &str {
    if cfg!(windows) && path.starts_with(r"\\?\") {
        &path[4..]
    } else {
        path
    }
}

/// Similar to [`normalize_for_display`]. But work on bytes.
pub fn normalize_for_display_bytes(path: &[u8]) -> &[u8] {
    if cfg!(windows) && path.starts_with(br"\\?\") {
        &path[4..]
    } else {
        path
    }
}
