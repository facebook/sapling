/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Low-level unix-domain-socket utilities.
//!
//! - Re-exports `std` or `uds_windows` types.
//! - Supports long (>107 bytes) paths by `chdir` temporarily.

#[cfg(unix)]
pub use std::os::unix::net;
use std::path::Path;

use fn_error_context::context;
pub use net::UnixListener;
pub use net::UnixStream;
#[cfg(windows)]
pub use uds_windows as net;

/// Bind to the unix domain socket for serving.
///
/// Side effect: changes the process's current directory temporarily.
/// This is to support long socket paths.
#[context("Binding unix domain socket at {}", path.display())]
pub fn bind(path: &Path) -> anyhow::Result<UnixListener> {
    maybe_with_chdir(path, |name| UnixListener::bind(name))
}

/// Connect to the unix domain socket.
///
/// Side effect: changes the process's current directory temporarily.
/// This is to support long socket paths.
#[context("Connecting to unix domain socket at {}", path.display())]
pub fn connect(path: &Path) -> anyhow::Result<UnixStream> {
    maybe_with_chdir(path, |name| UnixStream::connect(name))
}

/// Chdir to the directory of `path`, if the `path` is too long for unix-domain-socket.
/// See `sun_path` in `struct sockaddr_un` in `sys/un.h` for the size limit (107 bytes).
///
/// If chdir is not needed, the full path is passed to `func`. Otherwise, the file name
/// is passed to `func`.
fn maybe_with_chdir<T, E: Into<anyhow::Error>>(
    path: &Path,
    func: impl FnOnce(&Path) -> Result<T, E>,
) -> anyhow::Result<T> {
    // Check sys/un.h for this number. Note C needs a tailing '\0' for end-of-string.
    const SOCKADDR_UN_SUN_PATH_SIZE: usize = 108;
    let dir = if path.as_os_str().len() >= SOCKADDR_UN_SUN_PATH_SIZE {
        path.parent()
    } else {
        None
    };

    let (restore_dir, rest_path) = match dir {
        Some(dir) => {
            let file_name = match path.file_name() {
                Some(name) => Path::new(name),
                None => path,
            };
            let restore_dir = std::env::current_dir()?;
            std::env::set_current_dir(dir)?;
            (Some(restore_dir), file_name)
        }
        None => (None, path),
    };

    let result = func(rest_path);

    if let Some(dir) = restore_dir {
        std::env::set_current_dir(dir)?;
    }

    result.map_err(Into::into)
}
