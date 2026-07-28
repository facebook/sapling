/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(target_os = "linux")]
use std::process::Command;

#[cfg(target_os = "linux")]
use anyhow::Result;
#[cfg(target_os = "linux")]
use anyhow::anyhow;

#[cfg(target_os = "linux")]
use crate::instance::EdenFsInstance;

#[cfg(target_os = "linux")]
const EDENFS_UNIT_NAME_TEMPLATE: &str = "edenfs@{escaped_state_dir}.service";

/// Compute the systemd unit name for this instance using `systemd-escape`.
///
/// Uses `systemd-escape --path` to produce the canonical path encoding for the
/// state directory.  Returns an error if `systemd-escape` is not available or fails.
#[cfg(target_os = "linux")]
pub fn get_systemd_unit(instance: &EdenFsInstance) -> Result<String> {
    let config_dir = instance.get_config_dir().to_string_lossy();
    let output = Command::new("systemd-escape")
        .arg("--path")
        .arg(config_dir.as_ref())
        .output()
        .map_err(|e| anyhow!("systemd-escape is not installed: {e}"))?;

    if !output.status.success() {
        return Err(anyhow!(
            "systemd-escape failed for {}: {}",
            config_dir,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let escaped = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(EDENFS_UNIT_NAME_TEMPLATE.replace("{escaped_state_dir}", &escaped))
}

/// D-Bus session environment (`XDG_RUNTIME_DIR`, `DBUS_SESSION_BUS_ADDRESS`)
/// for reaching the current user's systemd `--user` manager, derived from the
/// current UID rather than from inherited environment variables.
///
/// Returns `None` when the D-Bus session socket for the current user does not
/// exist.
#[cfg(target_os = "linux")]
pub fn systemd_user_session_env() -> Option<[(&'static str, String); 2]> {
    // SAFETY: `getuid` takes no arguments and touches no memory; POSIX
    // guarantees it always succeeds, so there are no preconditions to uphold.
    let uid = unsafe { libc::getuid() };
    let runtime_dir = format!("/run/user/{uid}");
    let bus_path = format!("{runtime_dir}/bus");

    if !std::path::Path::new(&bus_path).exists() {
        return None;
    }

    Some([
        ("XDG_RUNTIME_DIR", runtime_dir),
        ("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={bus_path}")),
    ])
}
