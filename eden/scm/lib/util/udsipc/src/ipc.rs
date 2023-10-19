/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Wraps low level uds in a higher level API for ease-of-use:
//! - Use `NodeIpc` to support structured messages and fd sending.
//! - Maintains file deletion transparently.

use std::io;
use std::path::Path;
use std::path::PathBuf;

use fs_err as fs;
use nodeipc::NodeIpc;

use crate::uds;
use crate::uds::UnixListener;

/// Serve at the given path.
///
/// Return a iterator that yields a new `NodeIpc` for each client.
/// Dropping the iterator deletes the unix domain socket.
pub fn serve(path: PathBuf) -> anyhow::Result<Incoming> {
    let _ = fs::remove_file(&path);
    let listener = uds::bind(&path)?;
    let private_path = path.with_extension("private");
    let incoming = Incoming {
        listener,
        path,
        private_path,
    };

    Ok(incoming)
}

/// Connect to the given path.
///
/// Delete dead (ECONNREFUSED) files automatically.
pub fn connect(path: &Path) -> anyhow::Result<NodeIpc> {
    let stream = match uds::connect(path) {
        Ok(stream) => stream,
        Err(e) => {
            if let Some(e) = e.downcast_ref::<io::Error>() {
                if e.kind() == io::ErrorKind::ConnectionRefused {
                    // Dead socket (server was killed? reboot?). Remove it.
                    let _ = fs::remove_file(path);
                }
            }
            return Err(e);
        }
    };

    // Wrap in NodeIpc for ease of use.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(None)?;
    let ipc = NodeIpc::from_socket(stream)?;

    Ok(ipc)
}

/// Similar to `std::net::Incoming` but:
/// - Owns `listener`. Does not use lifetime.
/// - Deletes the domain sockets on drop.
/// - Provides `get_is_alive_func()` to check if the socket file is still on disk.
pub struct Incoming {
    listener: UnixListener,
    path: PathBuf,
    private_path: PathBuf,
}

impl Incoming {
    /// Get a function to check if the socket file is still on disk.
    /// This can be useful to decide whether to exit in a loop.
    pub fn get_is_alive_func(&self) -> Box<dyn (Fn() -> bool) + Send + Sync + 'static> {
        let path = self.path.clone();
        let private_path = self.private_path.clone();
        Box::new(move || path.exists() || private_path.exists())
    }
}

impl Iterator for Incoming {
    type Item = NodeIpc;

    fn next(&mut self) -> Option<Self::Item> {
        let stream = self.listener.accept().ok()?.0;
        stream.set_read_timeout(None).ok()?;
        stream.set_write_timeout(None).ok()?;
        stream.set_nonblocking(false).ok()?;
        let ipc = NodeIpc::from_socket(stream).ok()?;
        Some(ipc)
    }
}

impl Drop for Incoming {
    fn drop(&mut self) {
        if fs::remove_file(&self.path).is_err() {
            let _ = fs::remove_file(&self.private_path);
        }
    }
}
