/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Utilities shared for the crate.

use std::fs;
use std::io;
use std::path::PathBuf;

use anyhow::Context;
use fn_error_context::context;
use once_cell::sync::Lazy;

// The socket directory and prefix contain identity and version
// so we can have multiple servers running with different
// identities. or versions, and we don't need to check versions
// and invalidate servers manually.
static SOCKET_DIR_NAME: Lazy<String> = Lazy::new(|| {
    let cli_name = identity::default().cli_name();
    format!("{}-cmdserver", cli_name)
});

static PREFIX: Lazy<String> = Lazy::new(|| {
    let short_version: &str =
        match version::VERSION.rsplit_once(|ch: char| !ch.is_ascii_alphanumeric()) {
            Some((_, rest)) => rest,
            None => version::VERSION,
        };
    // Include number of groups in prefix so long running processes
    // with different groups can co-exist with new processes.
    let ngroups = groups_count();
    if ngroups == 0 {
        short_version.to_string()
    } else {
        format!("{}n{}", short_version, ngroups)
    }
});

/// Return the "prefix" useful as the prefix of the uds files.
pub(crate) fn prefix() -> &'static str {
    &PREFIX
}

/// Create and return a runtime directory intended for uds files.
/// The directory contains `SOCKET_DIR_NAME` in its path.
#[context("Creating a runtime directory")]
pub(crate) fn runtime_dir() -> anyhow::Result<PathBuf> {
    let parent = match dirs::runtime_dir().or_else(|| {
        // ~/.local/share, AppData\Local
        dirs::data_local_dir().map(|local| local.join("CommandServer"))
    }) {
        None => {
            #[allow(unused_mut)]
            let mut dir = std::env::temp_dir();
            #[cfg(unix)]
            {
                // temp_dir() is usually insecure, globally writable on *nix.
                // Try to create a directory with 0o700 permission in it.
                use std::fs::DirBuilder;
                use std::os::unix::fs::DirBuilderExt;

                let mut builder = DirBuilder::new();
                dir = dir.join(format!("uid-{}", unsafe { libc::getuid() }));
                match builder.mode(0o700).create(&dir) {
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => {
                        return Err(e).with_context(|| {
                            format!("Creating a user exclusive tmpdir at {}", dir.display())
                        });
                    }
                    Ok(_) => {}
                }
            }
            dir
        }
        Some(dir) => dir,
    };

    let dir = parent.join(&*SOCKET_DIR_NAME);
    match fs::create_dir_all(&dir) {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(e).with_context(|| format!("Creating a directory at {}", dir.display()));
        }
        Ok(_) => {}
    }

    Ok(dir)
}

/// Get the number of groups.
fn groups_count() -> usize {
    #[cfg(unix)]
    {
        let mut ngroups: libc::c_int = 0;
        if unsafe { libc::getgroups(0, std::ptr::null_mut()) } == -1 {
            ngroups = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
            if ngroups <= 0 {
                return 0;
            }
        }
        return ngroups as usize;
    }

    #[allow(unreachable_code)]
    0
}

/// Get a sorted list of group ids on POSIX.
///
/// If the client and the server have different lists of groups,
/// then the server should not serve the client.
pub fn groups() -> Option<Vec<u32>> {
    #[cfg(unix)]
    {
        let ngroups = groups_count();
        let mut groups: Vec<libc::gid_t> = vec![0; ngroups];
        let ngroups = unsafe { libc::getgroups(ngroups as _, groups.as_mut_ptr()) };
        if ngroups < 0 {
            return None;
        }

        groups.truncate(ngroups as _);
        groups.sort_unstable();
        return Some(groups.into_iter().map(|v| v).collect());
    }

    #[allow(unreachable_code)]
    None
}

/// Get the `RLIMIT_NOFILE` limit on POSIX.
///
/// If the client has a higher limit than the server, then the server
/// should not serve the client.
pub fn rlimit_nofile() -> Option<u64> {
    #[cfg(unix)]
    {
        let mut rlim: libc::rlimit = unsafe { std::mem::zeroed() };
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) } == 0 {
            return Some(rlim.rlim_cur as _);
        }
    }

    None
}

/// Get the umask on POSIX.
pub fn get_umask() -> Option<u32> {
    #[cfg(unix)]
    unsafe {
        let mask = libc::umask(0);
        libc::umask(mask);
        return Some(mask as _);
    }
    #[allow(unreachable_code)]
    None
}
