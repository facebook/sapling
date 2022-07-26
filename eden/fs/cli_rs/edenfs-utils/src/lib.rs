/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! EdenFS utils

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use subprocess::Exec;
use subprocess::Redirection;

pub mod humantime;
pub mod metadata;

pub fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf> {
    Ok(PathBuf::from(std::str::from_utf8(bytes).from_err()?))
}

pub fn bytes_from_path(path: PathBuf) -> Result<Vec<u8>> {
    Ok(path
        .into_os_string()
        .into_string()
        .map_err(|e| EdenFsError::Other(anyhow!("invalid checkout path {:?}", e)))?
        .as_bytes()
        .to_vec())
}

const ENV_KEYS_TO_REMOVE: &[&str] = &[
    "DYLD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "PAR_LAUNCH_TIMESTAMP",
];
// some processes like hg and arc are sensitive about their environments, we
// clear variables that might make problems for their dynamic linking.
// note buck is even more sensitive see buck.run_buck_command
//
// Clean out par related environment so that we don't cause problems
// for our child process
pub fn get_environment_suitable_for_subprocess() -> Vec<(OsString, OsString)> {
    std::env::vars()
        .filter_map(|(k, v)| {
            if ENV_KEYS_TO_REMOVE.contains(&k.as_str())
                || k.starts_with("FB_PAR")
                || k.starts_with("PYTHON")
            {
                None
            } else {
                Some((k.into(), v.into()))
            }
        })
        .collect()
}

/// In the EdenFS buck integration tests we build buck from source
/// in these tests we need to use the source built buck. The path for
/// this will be in the SOURCE_BUILT_BUCK environment variable. Otherwise we use
/// the default buck in our path.
pub fn get_buck_command() -> String {
    match std::env::vars().find(|(k, _)| k == "SOURCE_BUILT_BUCK") {
        Some((_, v)) => v,
        None => "buck".to_string(),
    }
}

/// Buck is sensitive to many environment variables, so we need to set them up
/// properly before calling into buck
pub fn get_env_with_buck_version(path: &Path) -> Result<Vec<(OsString, OsString)>> {
    let mut env = get_environment_suitable_for_subprocess();
    // If we are going to use locally built buck we don't need to set a buck
    // version. The locally build buck will only use the locally built
    // version
    if env
        .iter()
        .find(|&(k, _)| k == "SOURCE_BUILT_BUCK")
        .is_none()
    {
        // Using BUCKVERSION=last here to avoid triggering a download of a new
        // version of buck just to kill off buck.  This is specific to Facebook's
        // deployment of buck, and has no impact on the behavior of the opensource
        // buck executable.
        let buck_version = if !cfg!(windows) {
            Ok("last".to_string())
        } else {
            // On Windows, "last" doesn't work, fallback to reading the .buck-java11 file.
            let output = Exec::cmd(get_buck_command())
                .arg("--version-fast")
                .stdout(Redirection::Pipe)
                .stderr(Redirection::Pipe)
                .cwd(path)
                .capture()
                .from_err()?;

            if output.success() {
                Ok(output.stdout_str().trim().to_string())
            } else {
                Err(EdenFsError::Other(anyhow!(
                    "Failed to execute command to get buck version, stderr: {}, exit status: {:?}",
                    output.stderr_str().trim(),
                    output.exit_status,
                )))
            }
        }?;
        env.push((OsString::from("BUCKVERSION"), OsString::from(buck_version)));
    }
    Ok(env)
}

#[cfg(windows)]
pub fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    path.to_string_lossy()
        .strip_prefix(r"\\?\")
        .map(From::from)
        .unwrap_or(path)
}
