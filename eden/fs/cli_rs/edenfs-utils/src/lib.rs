/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! EdenFS utils

use std::env::var;
use std::ffi::OsString;
use std::fs::read_to_string;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use glob::glob;
use subprocess::Exec;
use subprocess::Redirection;
use sysinfo::Pid;
use sysinfo::ProcessExt;
use sysinfo::SystemExt;
use tracing::trace;

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
    if let Ok(buck_cmd) = var("SOURCE_BUILT_BUCK") {
        buck_cmd
    } else {
        "buck".to_string()
    }
}

/// Buck is sensitive to many environment variables, so we need to set them up
/// properly before calling into buck
pub fn get_env_with_buck_version(path: &Path) -> Result<Vec<(OsString, OsString)>> {
    let mut env = get_environment_suitable_for_subprocess();
    // If we are going to use locally built buck we don't need to set a buck
    // version. The locally build buck will only use the locally built
    // version

    // TODO(T135622175): setting `BUCKVERSION=last` has caused issues with repos that are checked
    // out to commits that are more than a few days old. For now, let's disable this code path.
    // This will hinder performance a bit, but it should make `buck kill` more reliable.
    if false
        && env
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

pub fn get_executable(pid: sysinfo::Pid) -> Option<PathBuf> {
    let mut system = sysinfo::System::new();

    if system.refresh_process(pid) {
        if let Some(process) = system.process(pid) {
            let executable = process.exe();
            trace!(%pid, ?executable, "found process executable");

            #[cfg(unix)]
            {
                // We may get a path ends with (deleted) if the executable is deleted on UNIX.
                let path = executable
                    .to_str()
                    .unwrap_or("")
                    .trim_end_matches(" (deleted)");
                return Some(path.into());
            }
            #[cfg(not(unix))]
            {
                return Some(executable.into());
            }
        } else {
            trace!(%pid, "unable to find process");
        }
    } else {
        trace!("unable to load process information");
    }

    None
}

pub fn is_process_running(pid: Pid) -> bool {
    let mut system = sysinfo::System::new();

    if system.refresh_process(pid) {
        system.process(pid).is_some()
    } else {
        false
    }
}

pub fn find_second_level_buck_projects(path: &Path) -> Result<Vec<PathBuf>> {
    /*
     * While repos usually have a top level buckconfig, in some cases projects have
     * their own configuration files one level down.  This glob() finds those directories for us.
     */
    let buck_configs = glob(&format!("{}/*/.buckconfig", path.to_string_lossy())).from_err()?;
    let buck_projects = buck_configs
        .filter_map(|c| match c {
            Ok(project) => {
                if project.is_file() {
                    project.parent().map(|p| p.to_owned())
                } else {
                    None
                }
            }
            Err(_) => None,
        })
        .collect();
    Ok(buck_projects)
}

/// Stop the major buckd instances that are likely to be running for path
pub fn stop_buckd_for_repo(path: &Path) {
    match find_second_level_buck_projects(path) {
        Ok(projects) => {
            for project in projects {
                if is_buckd_running_for_path(&project) {
                    if let Err(e) = stop_buckd_for_path(&project) {
                        eprintln!(
                            "Failed to kill buck. Please manually run `buck kill` in `{}`\n{:?}\n\n",
                            &project.display(),
                            e
                        )
                    }
                }
            }
        }
        Err(_) => {}
    };
}

pub fn is_buckd_running_for_path(path: &Path) -> bool {
    let pid_file = path.join(".buckd").join("pid");
    let file_contents = read_to_string(&pid_file).unwrap_or_default();

    if let Ok(buck_pid) = file_contents.trim().parse::<Pid>() {
        is_process_running(buck_pid)
    } else {
        false
    }
}

/// Buck is sensitive to many environment variables, so we need to set them up
/// properly before calling into buck. Use this function to guarantee environment
/// variables are set up correctly.
pub fn run_buck_command(buck_command: &mut Command, path: &Path) -> Result<Output> {
    let buck_envs = get_env_with_buck_version(path)?;
    buck_command
        .envs(buck_envs)
        .current_dir(path)
        .output()
        .from_err()
}

pub fn stop_buckd_for_path(path: &Path) -> Result<()> {
    println!("Stopping buck in {}...", path.display());
    let mut kill_cmd = Command::new(get_buck_command());
    kill_cmd.arg("kill");
    let can_path = path.canonicalize().from_err()?;
    let out = run_buck_command(&mut kill_cmd, &can_path)?;
    if out.status.success() {
        Ok(())
    } else {
        Err(EdenFsError::Other(anyhow!(
            "Failed to kill buck, stderr: {}, exit status: {:?}",
            String::from_utf8_lossy(&out.stderr),
            out.status,
        )))
    }
}

#[cfg(windows)]
pub fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    path.to_string_lossy()
        .strip_prefix(r"\\?\")
        .map(From::from)
        .unwrap_or(path)
}

#[cfg(unix)]
/// on Unixy platforms, all symlinks are files and must be removed with std::fs::remove_file
pub fn remove_symlink(path: &Path) -> Result<()> {
    std::fs::remove_file(path).from_err()?;
    Ok(())
}

#[cfg(windows)]
/// on Windows, directory symlinks must be removed with std::fs::remove_dir instead.
pub fn remove_symlink(path: &Path) -> Result<()> {
    std::fs::remove_dir(path).from_err()?;
    Ok(())
}

#[cfg(not(any(windows, unix)))]
/// on other platforms, we don't know how to handle removing symlinks. Panic instead of guessing
pub fn remove_symlink(path: &Path) -> Result<()> {
    panic!("failed to remove symlink, unsupported platform");
}
