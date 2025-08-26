/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Connection "pool" by having multiple uds files in a directory.

use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use fn_error_context::context;
use fs_err as fs;
use nodeipc::NodeIpc;

use crate::ipc;

macro_rules! or {
    // `$rest` could be: `continue`, `return None`.
    ($result:expr, $($rest:tt)*) => {
        match $result {
            Ok(value) => value,
            Err(_) => $($rest)*,
        }
    };
}

/// Serve in the given directory, with the given prefix.
///
/// The callsite might want to use `is_alive` to check if it should exit.
#[context("Serving at directory {}", dir.display())]
pub fn serve(dir: &Path, prefix: &str) -> anyhow::Result<ipc::Incoming> {
    fs_err::create_dir_all(dir)?;
    let path = dir.join(format!("{}-{}", prefix, std::process::id()));
    ipc::serve(path)
}

/// Connect to any server uds in the given directory.
///
/// If `exclusive` is set, the uds file is first renamed to ".private"
/// to ensure that one uds file only serves one client.
#[context("Connecting to any server socket in {}", dir.display())]
pub fn connect(dir: &Path, prefix: &str, exclusive: bool) -> anyhow::Result<NodeIpc> {
    let mut attempts = Vec::new();

    for mut path in list_uds_paths(dir, prefix) {
        if exclusive {
            path = or!(path.exclusive(), continue);
        }
        match path.connect() {
            Ok(ipc) => return Ok(ipc),
            Err(e) => {
                attempts.push(e);
            }
        };
    }

    anyhow::bail!(
        "Failed to connect to any uds files in {}. Attempted: {:?}",
        dir.display(),
        attempts,
    )
}

/// Unix-domain-socket path that can potentially be connected.
pub struct ConnectablePath {
    pub(crate) path: PathBuf,
}

impl ConnectablePath {
    /// Connect to this path.
    pub fn connect(self) -> anyhow::Result<NodeIpc> {
        let result = ipc::connect(&self.path);
        if self.path.extension().unwrap_or_default() == "private" {
            let _ = fs::remove_file(&self.path);
        }
        result
    }

    /// Make the path exclusive by renaming the file to `.private`.
    pub fn exclusive(mut self) -> anyhow::Result<Self> {
        let path = &self.path;
        let new_path = path.with_extension("private");
        fs::rename(path, &new_path)?;
        self.path = new_path;
        Ok(self)
    }
}

/// List uds paths that are potentially connectable in a directory.
/// The files are started with the given prefix.
///
/// Errors are ignored.
pub fn list_uds_paths<'a>(
    dir: &Path,
    prefix: &'a str,
) -> Box<dyn Iterator<Item = ConnectablePath> + Send + 'a> {
    let dir = or!(fs::read_dir(dir), return Box::new(std::iter::empty()));

    let iter = dir.filter_map(move |entry| {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let name = name.to_str().unwrap_or_default();
        if !name.starts_with(prefix) {
            // Remove stale uds files if they are older than 12 hours.
            let _ = maybe_remove_stale_file(&entry, Duration::from_secs(43200));
            return None;
        }

        let path = entry.path();

        // Skip ".lock" files.
        if path.extension().unwrap_or_default() == "lock" {
            return None;
        }

        // "Taken" by other process?
        if path.extension().unwrap_or_default() == "private" {
            // Remove stale files.
            // Rename bumps "accessed" time. Use it to detect stale files.
            let _ = maybe_remove_stale_file(&entry, Duration::from_secs(60));
            return None;
        }

        Some(ConnectablePath { path })
    });

    Box::new(iter)
}

fn maybe_remove_stale_file(entry: &fs::DirEntry, duration: Duration) -> io::Result<()> {
    let metadata = entry.metadata()?;
    let accessed = metadata.accessed()?;
    if accessed.elapsed().unwrap_or_default() > duration {
        fs::remove_file(entry.path())?;
    }
    Ok(())
}
