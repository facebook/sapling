/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use serde::*;
use sha2::Digest;
use sha2::Sha256;

// Take care with the full path to the utility so that we are not so easily
// tricked into running something scary if we are setuid root.
pub const DISKUTIL: &str = "/usr/sbin/diskutil";

#[derive(Clone, Debug, PartialEq, Eq)]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ApfsContainer {
    pub container_reference: String,
    pub volumes: Vec<ApfsVolume>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ApfsVolume {
    pub device_identifier: String,
    pub name: Option<String>,
}

impl ApfsVolume {
    /// Resolve the current mount point for this volume by looking
    /// at the mount table.  The mount table is optional; if not
    /// provided by the caller, this function will resolve it for
    /// itself.
    /// If you are resolving more than mount point in a loop, then
    /// it is preferable to pass in the mount table so that it isn't
    /// recomputed on each call.
    pub fn get_current_mount_point(&self, table: Option<&MountTable>) -> Option<String> {
        let table = MountTable::parse_if_needed(table).ok()?;
        let dev_name = format!("/dev/{}", self.device_identifier);
        for entry in table.entries {
            if entry.device == dev_name {
                return Some(entry.mount_point);
            }
        }
        None
    }

    /// If this volume was created by this tool, return its preferred
    /// (rather than current) mount point.
    pub fn preferred_mount_point(&self) -> Option<String> {
        if self.is_edenfs_managed_volume() {
            let name = self.name.as_ref().unwrap();
            Some(name[7..].to_owned())
        } else {
            None
        }
    }

    /// Returns true if the volume name matches our "special" edenfs managed
    /// volume name pattern.
    pub fn is_edenfs_managed_volume(&self) -> bool {
        self.name
            .as_ref()
            .map_or(false, |name| name.starts_with("edenfs:"))
    }

    /// Returns true if this is an edenfs managed volume and if the provided
    /// current mount point path is the preferred location.
    /// The intent is that current is produced by calling `get_current_mount_point`
    /// and then passed here.
    pub fn is_preferred_location(&self, current: &str) -> Result<bool> {
        let preferred = self
            .preferred_mount_point()
            .ok_or_else(|| anyhow!("this volume is not an edenfs managed volume"))?;
        Ok(preferred == current)
    }

    /// Returns true if this is an edenfs managed volume and if the preferred location
    /// is inside the provided checkout path.
    pub fn is_preferred_checkout(&self, checkout: &str) -> Result<bool> {
        let preferred = self
            .preferred_mount_point()
            .ok_or_else(|| anyhow!("this volume is not an edenfs managed volume"))?;
        // Append "/" as checkouts can have the same prefix, e.g. fbsource, fbsource2
        Ok(preferred.starts_with(&(checkout.to_string() + "/")))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountEntry {
    device: String,
    mount_point: String,
}

impl MountEntry {
    pub fn new(device: &str, mount_point: &str) -> Self {
        Self {
            device: device.to_owned(),
            mount_point: mount_point.to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountTable {
    pub entries: Vec<MountEntry>,
}

impl MountTable {
    pub fn parse_mount_table_text(text: &str) -> Self {
        let mut entries = vec![];
        for line in text.lines() {
            // For entries that have spaces in the mount point name,
            // the mount command doesn't do any kind of helpful escaping.
            // The entries that we care about have the form:
            // <DEVICE><SPACE>on<SPACE><PATH WITH OPTIONAL SPACES>(OPTIONS)
            // We trim off the options and split around ` on ` so that we just
            // have two simple fields to work with, and won't need to consider
            // spaces.
            let mut iter = line.rsplitn(2, " (");
            // Discard the options
            let _options = iter.next();
            if let Some(lhs) = iter.next() {
                let mut iter = lhs.split(" on ");
                match (iter.next(), iter.next()) {
                    (Some(device), Some(mount_point)) => {
                        entries.push(MountEntry::new(device, mount_point));
                    }
                    _ => {}
                }
            }
        }

        Self { entries }
    }

    pub fn parse_system_mount_table() -> Result<Self> {
        let output = new_cmd_unprivileged("/sbin/mount").output()?;
        if !output.status.success() {
            bail!("failed to execute mount: {:#?}", output);
        }
        Ok(Self::parse_mount_table_text(&String::from_utf8(
            output.stdout,
        )?))
    }

    fn parse_if_needed(existing: Option<&Self>) -> Result<Self> {
        if let Some(table) = existing {
            Ok(table.clone())
        } else {
            Self::parse_system_mount_table()
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Containers {
    pub containers: Vec<ApfsContainer>,
}

// A note about `native-plist` vs `json-plist`.
// The intent is that `native-plist` be the thing that we use for real in the long
// term, but we are currently blocked from using this in our CI system due to some
// vendoring issues with external crates.  For the sake of unblocking this feature
// the `json-plist` feature (which is the default) uses a `plutil` executable on
// macos to convert the plist to json and then uses serde_json to extract the data
// of interest.
// In the near future we should unblock the vendoring issue and will be able to
// remove the use of plutil.

#[cfg(feature = "json-plist")]
pub fn parse_plist<T: de::DeserializeOwned>(data: &str) -> Result<T> {
    use std::io::Read;
    use std::io::Write;

    // Run plutil and tell it to convert stdin (that last `-` arg)
    // into json and output it to stdout (the `-o -`).
    let child = new_cmd_unprivileged("/usr/bin/plutil")
        .args(&["-convert", "json", "-o", "-", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    let mut input = child.stdin.unwrap();
    input.write_all(data.as_bytes())?;
    drop(input);

    let mut json = String::new();
    child.stdout.unwrap().read_to_string(&mut json)?;

    serde_json::from_str(&json).context("parsing json data")
}

#[cfg(feature = "native-plist")]
pub fn parse_plist<T>(data: &str) -> Result<T> {
    plist::from_bytes(data.as_bytes()).context("parsing plist data")
}

/// Obtain the list of apfs containers and volumes by executing `diskutil`.
pub fn apfs_list() -> Result<Vec<ApfsContainer>> {
    let output = new_cmd_unprivileged(DISKUTIL)
        .args(&["apfs", "list", "-plist"])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("failed to execute diskutil list: {:#?}", output);
    }
    Ok(parse_plist::<Containers>(&String::from_utf8(output.stdout)?)?.containers)
}

pub fn find_existing_volume<'a>(
    containers: &'a [ApfsContainer],
    name: &str,
) -> Option<&'a ApfsVolume> {
    for container in containers {
        for volume in &container.volumes {
            if volume.name.as_ref().map(String::as_ref) == Some(name) {
                return Some(volume);
            }
        }
    }
    None
}

/// Prepare a command to be run with no special privs.
/// We're usually installed setuid root so we already have privs; the
/// command invocation will restore the real uid/gid of the caller
/// as part of running the command so that we avoid running too much
/// stuff with privs.
pub fn new_cmd_unprivileged(path: &str) -> Command {
    let path: PathBuf = path.into();
    assert!(path.is_absolute());
    let mut cmd = Command::new(path);

    if geteuid() == 0 {
        // We're running with effective root privs; run this command
        // with the privs of the real user, just in case.
        cmd.uid(getuid()).gid(getgid());
    }

    cmd
}

fn getgid() -> u32 {
    unsafe { libc::getgid() }
}

pub fn getuid() -> u32 {
    unsafe { libc::getuid() }
}

pub fn geteuid() -> u32 {
    unsafe { libc::geteuid() }
}

pub fn canonicalize_mount_point_path(mount_point: &str) -> Result<String> {
    let canon = std::fs::canonicalize(mount_point)
        .with_context(|| format!("canonicalizing path {}", mount_point))?;
    canon
        .to_str()
        .ok_or_else(|| anyhow!("path {} somehow isn't unicode on macOS", canon.display()))
        .map(str::to_owned)
}

/// Hash the subdirectory of mount point. In practice this is used to avoid
/// an error with APFS volume name constraints.
pub fn encode_canonicalized_path<P: AsRef<Path>>(mount_point: P) -> String {
    format!(
        "{:x}",
        Sha256::digest(mount_point.as_ref().to_str().unwrap().as_bytes())
    )
}

/// Encode a mount point as a volume name.
/// The story here is that diskutil allows any user to create an APFS
/// volume, but requires root privs to mount it into the VFS.
/// We're setuid root to facilitate this, but to make things safe(r)
/// we create volumes with an encoded name so that we can tell that
/// they were created by this tool for a specific mount point.
/// We will only mount volumes that have that encoded name, at the
/// location encoded by their name and refuse to mount anything else.
pub fn encode_mount_point_as_volume_name<P: AsRef<Path>>(mount_point: P) -> String {
    let full_volume_name = format!("edenfs:{}", mount_point.as_ref().display());

    if full_volume_name.chars().count() > 127 {
        let hashed_mount = encode_canonicalized_path(&mount_point);
        return format!("edenfs:{}", hashed_mount);
    }

    full_volume_name
}

pub fn delete_volume(volume_name: &str) -> Result<()> {
    let containers = apfs_list()?;
    if let Some(volume) = find_existing_volume(&containers, volume_name) {
        // This will implicitly unmount, so we don't need to deal
        // with that here
        let output = new_cmd_unprivileged(DISKUTIL)
            .args(&["apfs", "deleteVolume", &volume.device_identifier])
            .output()?;
        if !output.status.success() {
            anyhow::bail!(
                "failed to execute diskutil deleteVolume {}: {:?}",
                volume.device_identifier,
                output
            );
        }
        Ok(())
    } else {
        bail!("Did not find a volume named {}", volume_name);
    }
}
