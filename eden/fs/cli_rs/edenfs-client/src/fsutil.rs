/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::path::Path;

#[cfg(target_os = "windows")]
fn release_files_in_dir(_dir: &Path) -> bool {
    // TODO: Implement release using handle.exe
    false
}

#[cfg(not(target_os = "windows"))]
fn release_files_in_dir(_dir: &Path) -> bool {
    false
}

pub fn forcefully_remove_dir_all(directory: &Path) -> std::io::Result<()> {
    let mut retries = 0;
    loop {
        if !directory.try_exists()? {
            // Path doesn't exist, either as a result of a previous work or it never did, so we're done.
            return Ok(());
        }
        let res = fs::remove_dir_all(directory);
        if res.is_ok() {
            // Successfully removed the directory and its contents, so we're done.
            return Ok(());
        }
        if retries >= 3 {
            // We've tried a few times, the directory refuses to die. Give up and return the error.
            return res;
        }
        release_files_in_dir(directory);
        retries += 1;
    }
}
