// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{fs, io::Write, path::Path, time::Instant};

/// Rename a path and write down error messages for investigation purpose.
pub(crate) fn debug_backup_error(path: &Path, error: failure::Error) -> failure::Fallible<()> {
    let backup_path = path.with_extension("bak");
    let error_path = backup_path.join("error.txt");
    if backup_path.exists() {
        // Only keep one backup. Just delete path.
        // This is racy. But it happens in a relatively rarely called debug-only
        // code path. So it's probably okay.
        if let Ok(mut file) = fs::OpenOptions::new().append(true).open(&error_path) {
            let _ = file.write_all(
                format!(
                    "[{:?}] Error: {:?} (not backed up)\n",
                    Instant::now(),
                    error
                )
                .as_bytes(),
            );
        }
        fs::remove_dir_all(path)?;
    } else {
        match fs::rename(path, &backup_path) {
            Ok(_) => {
                // Also write down error message.
                let _ = fs::write(
                    &error_path,
                    format!("[{:?}] Error: {:?} (this backup)\n", Instant::now(), error),
                );
            }
            Err(_) => {
                fs::remove_dir_all(path)?;
            }
        }
    }
    Ok(())
}
