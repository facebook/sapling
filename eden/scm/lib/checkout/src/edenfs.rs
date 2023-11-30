/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fs::read_to_string;
#[cfg(unix)]
use std::os::unix::prelude::MetadataExt;
use std::process::Command;

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use io::IO;
use spawn_ext::CommandExt;
use types::RepoPath;
use workingcopy::workingcopy::WorkingCopy;

/// run `edenfsctl redirect fixup`, potentially in background.
///
/// If the `.eden-redirections` file does not exist in the working copy,
/// or is empty, run nothing.
///
/// Otherwise, parse the fixup directories, if they exist and look okay,
/// run `edenfsctl redirect fixup` in background. This reduces overhead
/// especially on Windows.
///
/// Otherwise, run in foreground. This is needed for automation that relies
/// on `checkout HASH` to setup critical repo redirections.
pub fn edenfs_redirect_fixup(io: &IO, config: &dyn Config, wc: &WorkingCopy) -> Result<()> {
    let is_okay = match is_edenfs_redirect_okay(wc).unwrap_or(Some(false)) {
        Some(r) => r,
        None => return Ok(()),
    };
    let arg0 = config.get_or("edenfs", "command", || "edenfsctl".to_owned())?;
    let args_raw = config.get_or("edenfs", "redirect-fixup", || "redirect fixup".to_owned())?;
    let args = args_raw.split_whitespace().collect::<Vec<_>>();
    let mut cmd0 = Command::new(arg0);
    let cmd = cmd0.args(args);
    if is_okay {
        cmd.spawn_detached()?;
    } else {
        io.disable_progress(true)?;
        let status = cmd.status();
        io.disable_progress(false)?;
        status?;
    }
    Ok(())
}

/// Whether the edenfs redirect directories look okay, or None if redirect is unnecessary.
fn is_edenfs_redirect_okay(wc: &WorkingCopy) -> Result<Option<bool>> {
    let vfs = wc.vfs();
    let mut redirections = HashMap::new();

    // Check edenfs-client/src/redirect.rs for the config paths and file format.
    let client_paths = vec![
        wc.vfs().root().join(".eden-redirections"),
        wc.eden_client()?.client_path().join("config.toml"),
    ];

    for path in client_paths {
        // Cannot use vfs::read as config.toml is outside of the working copy
        let text = match read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                tracing::debug!("is_edenfs_redirect_okay failed to check: {}", e);
                return Ok(Some(false));
            }
        };
        if let Ok(s) = toml::from_str::<toml::Table>(text.as_str()) {
            if let Some(r) = s.get("redirections").and_then(|v| v.as_table()) {
                for (k, v) in r.iter() {
                    redirections.insert(k.to_owned(), v.to_string());
                }
            }
        }
    }

    if redirections.is_empty() {
        return Ok(None);
    }

    #[cfg(unix)]
    let root_device_inode = vfs.metadata(RepoPath::empty())?.dev();
    for (path, kind) in redirections.into_iter() {
        let path_metadata = match vfs.metadata(RepoPath::from_str(path.as_str())?) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if cfg!(windows) || kind == "symlink" {
            // kind is "bind" or "symlink". On Windows, "bind" is not supported
            if !path_metadata.is_symlink() {
                return Ok(Some(false));
            }
        } else {
            #[cfg(unix)]
            // Bind mount should have a different device inode
            if path_metadata.dev() == root_device_inode {
                return Ok(Some(false));
            }
        }
    }

    Ok(Some(true))
}
