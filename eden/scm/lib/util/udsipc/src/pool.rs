/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Connection "pool" by having multiple uds files in a directory.

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use fn_error_context::context;
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

/// Serve in the given directory.
///
/// The callsite might want to use `is_alive` to check if it should exit.
#[context("Serving at directory {}", dir.display())]
pub fn serve(dir: &Path) -> anyhow::Result<ipc::Incoming> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("server-{}", std::process::id()));
    ipc::serve(path)
}

/// Connect to any server uds in the given directory.
///
/// If `exclusive` is set, the uds file is first renamed to ".private"
/// to ensure that one uds file only serves one client.
#[context("Connecting to any server socket in {}", dir.display())]
pub fn connect(dir: &Path, exclusive: bool) -> anyhow::Result<NodeIpc> {
    let mut attempts = Vec::new();

    for mut path in list_uds_paths(dir) {
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
        ipc::connect(&self.path)
    }

    /// Make the path exclusive by renaming the file to `.private`.
    pub fn exclusive(mut self) -> anyhow::Result<Self> {
        let path = &self.path;
        let new_path = path.with_extension(".private");
        fs::rename(path, &new_path)?;
        self.path = new_path;
        Ok(self)
    }
}

/// List uds paths that are potentially connectable in a directory.
///
/// Errors are ignored.
pub fn list_uds_paths(dir: &Path) -> Box<dyn Iterator<Item = ConnectablePath> + Send> {
    let dir = or!(fs::read_dir(dir), return Box::new(std::iter::empty()));

    let iter = dir.filter_map(|entry| {
        let entry = entry.ok()?;
        let path = entry.path();

        // Skip ".lock" files.
        if path.extension().unwrap_or_default() == "lock" {
            return None;
        }

        // "Taken" by other process?
        if path.extension().unwrap_or_default() == "private" {
            // Remove stale files.
            // Rename bumps "accessed" time. Use it to detect stale files.
            let metadata = entry.metadata().ok()?;
            let accessed = metadata.accessed().ok()?;
            if accessed.elapsed().unwrap_or_default() > Duration::from_secs(60) {
                let _ = fs::remove_file(path);
            }
            return None;
        }

        Some(ConnectablePath { path })
    });

    Box::new(iter)
}
