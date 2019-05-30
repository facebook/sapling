// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#[cfg(not(unix))]
use std::fs::rename;
use std::{fs::remove_file as fs_remove_file, path::Path};

use failure::Fallible;
#[cfg(not(unix))]
use tempfile::Builder;

/// Remove the file pointed by `path`.
#[cfg(unix)]
pub fn remove_file<P: AsRef<Path>>(path: P) -> Fallible<()> {
    fs_remove_file(path)?;
    Ok(())
}

/// Remove the file pointed by `path`.
///
/// On Windows, removing a file can fail for various reasons, including if the file is memory
/// mapped. This can happen when the repository is accessed concurrently while a background task is
/// trying to remove a packfile. To solve this, we can rename the file before trying to remove it.
/// If the remove operation fails, a future repack will clean it up.
#[cfg(not(unix))]
pub fn remove_file<P: AsRef<Path>>(path: P) -> Fallible<()> {
    let path = path.as_ref();
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map_or(".to-delete".to_owned(), |ext| ".".to_owned() + ext + "-tmp");

    let dest_path = Builder::new()
        .prefix("")
        .suffix(&extension)
        .rand_bytes(8)
        .tempfile_in(path.parent().unwrap())?
        .into_temp_path();

    rename(path, &dest_path)?;

    // Ignore errors when removing the file, it will be cleaned up at a later time.
    let _ = fs_remove_file(dest_path);
    Ok(())
}
