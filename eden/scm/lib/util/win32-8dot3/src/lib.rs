/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Inspect and remove Windows 8.3 short names from files and directories.

use std::ffi::OsString;
use std::io;
use std::path::Path;

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use winapi;

/// Return the 8.3 short name of `path`, if it has one.
///
/// This returns `None` on non-Windows platforms.
pub fn short_name_at(path: &Path) -> io::Result<Option<OsString>> {
    #[cfg(windows)]
    return windows::short_name_at(path);

    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(None)
    }
}

/// Remove the 8.3 short name of `path`, if it has one.
///
/// Note the actual removal requires DELETE access, so it can fail if the path
/// is already opened in a way that prevents new DELETE access (e.g. by
/// NoFollowRoot).
///
/// This is a no-op on non-Windows platforms.
pub fn remove_short_name_at(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    return windows::remove_short_name_at(path);

    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_short_name_is_noop_without_short_name() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("file");
        std::fs::write(&path, [])?;
        remove_short_name_at(&path)
    }
}
