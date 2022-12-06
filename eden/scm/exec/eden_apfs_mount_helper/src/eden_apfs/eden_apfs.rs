/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::str;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use once_cell::sync::OnceCell;
use serde::de::DeserializeOwned;
use serde::*;
use sha2::Digest;
use sha2::Sha256;

// Take care with the full path to the utility so that we are not so easily
// tricked into running something scary if we are setuid root.
pub const DISKUTIL_PATH: &str = "/usr/sbin/diskutil";
pub const MOUNT_PATH: &str = "/sbin/mount";

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
    pub fn get_current_mount_point<T: SystemCommand>(
        &self,
        mount: &T,
        table: Option<&MountTable>,
    ) -> Option<String> {
        let table = MountTable::parse_if_needed(mount, table).ok()?;
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

    pub fn parse_system_mount_table<T: SystemCommand>(mount: &T) -> Result<Self> {
        let output = mount.run_unprivileged(&[])?;
        if !output.status.success() {
            bail!("failed to execute mount: {:#?}", output);
        }
        Ok(Self::parse_mount_table_text(&String::from_utf8(
            output.stdout,
        )?))
    }

    fn parse_if_needed<T: SystemCommand>(mount: &T, existing: Option<&Self>) -> Result<Self> {
        if let Some(table) = existing {
            Ok(table.clone())
        } else {
            Self::parse_system_mount_table(mount)
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Containers {
    pub containers: Vec<ApfsContainer>,
}

pub fn parse_plist<T: DeserializeOwned>(data: &str) -> Result<T> {
    plist::from_bytes(data.as_bytes()).context("parsing plist data")
}

pub trait SystemCommand {
    fn run_unprivileged(&self, args: &[&str]) -> Result<Output, std::io::Error>;
}

pub struct SystemCommandImpl(pub PathBuf);

impl SystemCommand for SystemCommandImpl {
    fn run_unprivileged(&self, args: &[&str]) -> Result<Output, std::io::Error> {
        new_cmd_unprivileged(&self.0).args(args).output()
    }
}

pub struct ApfsUtil<T: SystemCommand = SystemCommandImpl> {
    diskutil: T,
    mount: T,
}

impl ApfsUtil<SystemCommandImpl> {
    pub fn new(diskutil_path: impl AsRef<Path>, mount_path: impl AsRef<Path>) -> Self {
        Self {
            diskutil: SystemCommandImpl(diskutil_path.as_ref().to_owned()),
            mount: SystemCommandImpl(mount_path.as_ref().to_owned()),
        }
    }

    pub fn global() -> &'static Self {
        static INSTANCE: OnceCell<ApfsUtil> = OnceCell::new();
        INSTANCE.get_or_init(|| Self::new(DISKUTIL_PATH, MOUNT_PATH))
    }
}

impl<T: SystemCommand> ApfsUtil<T> {
    /// Obtain the list of apfs containers and volumes by executing `diskutil`.
    pub fn list_containers(&self) -> Result<Vec<ApfsContainer>> {
        let output = self
            .diskutil
            .run_unprivileged(&["apfs", "list", "-plist"])?;
        if !output.status.success() {
            anyhow::bail!("failed to execute diskutil list: {:#?}", output);
        }
        Ok(parse_plist::<Containers>(&String::from_utf8(output.stdout)?)?.containers)
    }

    fn list_stale_volumes_impl(
        &self,
        is_stale_predicate: impl Fn(&ApfsVolume) -> bool,
    ) -> Result<Vec<ApfsVolume>> {
        let containers = self.list_containers()?;
        let mount_table = MountTable::parse_system_mount_table(&self.mount)?;

        let mut stale_volumes = vec![];
        for container in containers {
            for vol in container.volumes {
                if !vol.is_edenfs_managed_volume()
                    || vol
                        .get_current_mount_point(&self.mount, Some(&mount_table))
                        .is_some()
                {
                    // ignore currently mounted or volumes not managed by EdenFS
                    continue;
                }

                if is_stale_predicate(&vol) {
                    stale_volumes.push(vol);
                }
            }
        }

        Ok(stale_volumes)
    }

    // Returns stale eden APFS redirection volumes based on checkout names.
    //
    // Note that because this implementation only takes into account checkout
    // names, it may incorrectly indicate that an unmounted redirection volume
    // with a hashed name is "stale", even if it is currently configured for
    // some checkout.
    pub fn list_stale_volumes_unsafe(&self, all_checkouts: &[String]) -> Result<Vec<ApfsVolume>> {
        self.list_stale_volumes_impl(|vol| {
            all_checkouts
                .iter()
                .try_fold(false, |acc, checkout| {
                    vol.is_preferred_checkout(checkout).map(|p| acc || p)
                })
                .map(std::ops::Not::not)
                .unwrap_or(false)
        })
    }

    // Returns stale eden APFS redirection volumes based on configured
    // redirections.
    pub fn list_stale_volumes(
        &self,
        configured_redirection_mount_points: &[PathBuf],
    ) -> Result<Vec<ApfsVolume>> {
        let mut configured_volume_names = HashSet::new();
        for mount_point in configured_redirection_mount_points {
            configured_volume_names.insert(encode_mount_point_as_volume_name(mount_point));
        }
        self.list_stale_volumes_impl(|vol| match &vol.name {
            Some(name) => !configured_volume_names.contains(name),
            None => false,
        })
    }

    pub fn delete_volume(&self, volume_name: &str) -> Result<()> {
        let containers = self.list_containers()?;
        if let Some(volume) = find_existing_volume(&containers, volume_name) {
            // This will implicitly unmount, so we don't need to deal
            // with that here
            let output = self.diskutil.run_unprivileged(&[
                "apfs",
                "deleteVolume",
                &volume.device_identifier,
            ])?;
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

    pub fn delete_scratch<P: AsRef<Path>>(&self, mount_point: P) -> Result<()> {
        let volume_name = encode_mount_point_as_volume_name(mount_point);
        self.delete_volume(volume_name.as_str())
    }

    pub fn unmount_scratch(
        &self,
        mount_point: &str,
        force: bool,
        mount_table: &MountTable,
    ) -> Result<()> {
        let containers = self.list_containers()?;

        for container in containers {
            for volume in &container.volumes {
                let preferred = match volume.preferred_mount_point() {
                    Some(path) => path,
                    None => continue,
                };

                if let Some(current_mount) =
                    volume.get_current_mount_point(&self.mount, Some(mount_table))
                {
                    if current_mount == mount_point || mount_point == preferred {
                        let mut args = vec!["unmount"];

                        if force {
                            args.push("force");
                        }
                        args.push(&volume.device_identifier);
                        let output = self.diskutil.run_unprivileged(&args)?;
                        if !output.status.success() {
                            anyhow::bail!(
                                "failed to execute diskutil unmount {}: {:?}",
                                volume.device_identifier,
                                output
                            );
                        }
                        return Ok(());
                    }
                }
            }
        }
        bail!("Did not find a volume mounted on {}", mount_point);
    }
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
pub fn new_cmd_unprivileged(path: impl AsRef<Path>) -> Command {
    assert!(path.as_ref().is_absolute());
    let mut cmd = Command::new(path.as_ref());

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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_mount_parse() {
        let data = r#"
/dev/disk1s1 on / (apfs, local, journaled)
devfs on /dev (devfs, local, nobrowse)
/dev/disk1s4 on /private/var/vm (apfs, local, noexec, journaled, noatime, nobrowse)
map -hosts on /net (autofs, nosuid, automounted, nobrowse)
map auto_home on /home (autofs, automounted, nobrowse)
eden@osxfuse0 on /Users/wez/fbsource (osxfuse_eden, nosuid, synchronous)
/dev/disk1s5 on /Users/wez/fbsource/buck-out (apfs, local, nodev, nosuid, journaled, nobrowse)
/dev/disk1s6 on /Users/wez/fbsource/fbcode/buck-out (apfs, local, nodev, nosuid, journaled, nobrowse)
/dev/disk1s7 on /Users/wez/fbsource/fbobjc/buck-out (apfs, local, nodev, nosuid, journaled, nobrowse)
/dev/disk1s8 on /private/tmp/wat the/woot (apfs, local, nodev, nosuid, journaled, nobrowse)
map -fstab on /Network/Servers (autofs, automounted, nobrowse)
/dev/disk1s9 on /private/tmp/parens (1) (apfs, local, nodev, nosuid, journaled, nobrowse)
"#;
        assert_eq!(
            MountTable::parse_mount_table_text(data).entries,
            vec![
                MountEntry::new("/dev/disk1s1", "/"),
                MountEntry::new("devfs", "/dev"),
                MountEntry::new("/dev/disk1s4", "/private/var/vm"),
                MountEntry::new("map -hosts", "/net"),
                MountEntry::new("map auto_home", "/home"),
                MountEntry::new("eden@osxfuse0", "/Users/wez/fbsource"),
                MountEntry::new("/dev/disk1s5", "/Users/wez/fbsource/buck-out"),
                MountEntry::new("/dev/disk1s6", "/Users/wez/fbsource/fbcode/buck-out"),
                MountEntry::new("/dev/disk1s7", "/Users/wez/fbsource/fbobjc/buck-out"),
                // This one has a space in the mount point path!
                MountEntry::new("/dev/disk1s8", "/private/tmp/wat the/woot"),
                MountEntry::new("map -fstab", "/Network/Servers"),
                MountEntry::new("/dev/disk1s9", "/private/tmp/parens (1)"),
            ]
        );
    }

    #[test]
    fn test_plist() {
        let data = r#"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
        <key>Containers</key>
        <array>
                <dict>
                        <key>APFSContainerUUID</key>
                        <string>C4AC89F6-8658-4857-972C-D485C213523A</string>
                        <key>CapacityCeiling</key>
                        <integer>499963174912</integer>
                        <key>CapacityFree</key>
                        <integer>30714478592</integer>
                        <key>ContainerReference</key>
                        <string>disk1</string>
                        <key>DesignatedPhysicalStore</key>
                        <string>disk0s2</string>
                        <key>Fusion</key>
                        <false/>
                        <key>PhysicalStores</key>
                        <array>
                                <dict>
                                        <key>DeviceIdentifier</key>
                                        <string>disk0s2</string>
                                        <key>DiskUUID</key>
                                        <string>2F978E12-5A2C-4EEB-BAE2-0E09CAEADC06</string>
                                        <key>Size</key>
                                        <integer>499963174912</integer>
                                </dict>
                        </array>
                        <key>Volumes</key>
                        <array>
                                <dict>
                                        <key>APFSVolumeUUID</key>
                                        <string>9AA7F3A4-A615-4F8D-91E3-F5C86D988D71</string>
                                        <key>CapacityInUse</key>
                                        <integer>461308219392</integer>
                                        <key>CapacityQuota</key>
                                        <integer>0</integer>
                                        <key>CapacityReserve</key>
                                        <integer>0</integer>
                                        <key>CryptoMigrationOn</key>
                                        <false/>
                                        <key>DeviceIdentifier</key>
                                        <string>disk1s1</string>
                                        <key>Encryption</key>
                                        <true/>
                                        <key>FileVault</key>
                                        <true/>
                                        <key>Locked</key>
                                        <false/>
                                        <key>Name</key>
                                        <string>Macintosh HD</string>
                                        <key>Roles</key>
                                        <array/>
                                </dict>
                                <dict>
                                        <key>APFSVolumeUUID</key>
                                        <string>A91FD4EA-684D-4122-9ACD-27E1465E99F6</string>
                                        <key>CapacityInUse</key>
                                        <integer>43061248</integer>
                                        <key>CapacityQuota</key>
                                        <integer>0</integer>
                                        <key>CapacityReserve</key>
                                        <integer>0</integer>
                                        <key>CryptoMigrationOn</key>
                                        <false/>
                                        <key>DeviceIdentifier</key>
                                        <string>disk1s2</string>
                                        <key>Encryption</key>
                                        <false/>
                                        <key>FileVault</key>
                                        <false/>
                                        <key>Locked</key>
                                        <false/>
                                        <key>Name</key>
                                        <string>Preboot</string>
                                        <key>Roles</key>
                                        <array>
                                                <string>Preboot</string>
                                        </array>
                                </dict>
                                <dict>
                                        <key>APFSVolumeUUID</key>
                                        <string>1C94FFC8-7649-470E-952D-16672E135C43</string>
                                        <key>CapacityInUse</key>
                                        <integer>510382080</integer>
                                        <key>CapacityQuota</key>
                                        <integer>0</integer>
                                        <key>CapacityReserve</key>
                                        <integer>0</integer>
                                        <key>CryptoMigrationOn</key>
                                        <false/>
                                        <key>DeviceIdentifier</key>
                                        <string>disk1s3</string>
                                        <key>Encryption</key>
                                        <false/>
                                        <key>FileVault</key>
                                        <false/>
                                        <key>Locked</key>
                                        <false/>
                                        <key>Name</key>
                                        <string>Recovery</string>
                                        <key>Roles</key>
                                        <array>
                                                <string>Recovery</string>
                                        </array>
                                </dict>
                                <dict>
                                        <key>APFSVolumeUUID</key>
                                        <string>6BC72964-0CA0-48AE-AAE1-7E9BFA8B2005</string>
                                        <key>CapacityInUse</key>
                                        <integer>6442676224</integer>
                                        <key>CapacityQuota</key>
                                        <integer>0</integer>
                                        <key>CapacityReserve</key>
                                        <integer>0</integer>
                                        <key>CryptoMigrationOn</key>
                                        <false/>
                                        <key>DeviceIdentifier</key>
                                        <string>disk1s4</string>
                                        <key>Encryption</key>
                                        <true/>
                                        <key>FileVault</key>
                                        <false/>
                                        <key>Locked</key>
                                        <false/>
                                        <key>Name</key>
                                        <string>VM</string>
                                        <key>Roles</key>
                                        <array>
                                                <string>VM</string>
                                        </array>
                                </dict>
                                <dict>
                                        <key>APFSVolumeUUID</key>
                                        <string>6C7EEDAD-385B-49AB-857B-AD15D98D13ED</string>
                                        <key>CapacityInUse</key>
                                        <integer>790528</integer>
                                        <key>CapacityQuota</key>
                                        <integer>0</integer>
                                        <key>CapacityReserve</key>
                                        <integer>0</integer>
                                        <key>CryptoMigrationOn</key>
                                        <false/>
                                        <key>DeviceIdentifier</key>
                                        <string>disk1s5</string>
                                        <key>Encryption</key>
                                        <true/>
                                        <key>FileVault</key>
                                        <false/>
                                        <key>Locked</key>
                                        <false/>
                                        <key>Name</key>
                                        <string>edenfs:/Users/wez/fbsource/buck-out</string>
                                        <key>Roles</key>
                                        <array/>
                                </dict>
                                <dict>
                                        <key>APFSVolumeUUID</key>
                                        <string>0DAB1407-0283-408E-88EE-CD41CE9E7BCA</string>
                                        <key>CapacityInUse</key>
                                        <integer>781156352</integer>
                                        <key>CapacityQuota</key>
                                        <integer>0</integer>
                                        <key>CapacityReserve</key>
                                        <integer>0</integer>
                                        <key>CryptoMigrationOn</key>
                                        <false/>
                                        <key>DeviceIdentifier</key>
                                        <string>disk1s6</string>
                                        <key>Encryption</key>
                                        <true/>
                                        <key>FileVault</key>
                                        <false/>
                                        <key>Locked</key>
                                        <false/>
                                        <key>Name</key>
                                        <string>edenfs:/Users/wez/fbsource/fbcode/buck-out</string>
                                        <key>Roles</key>
                                        <array/>
                                </dict>
                                <dict>
                                        <key>APFSVolumeUUID</key>
                                        <string>253A48CA-074E-496E-9A62-9F64831D7A65</string>
                                        <key>CapacityInUse</key>
                                        <integer>925696</integer>
                                        <key>CapacityQuota</key>
                                        <integer>0</integer>
                                        <key>CapacityReserve</key>
                                        <integer>0</integer>
                                        <key>CryptoMigrationOn</key>
                                        <false/>
                                        <key>DeviceIdentifier</key>
                                        <string>disk1s7</string>
                                        <key>Encryption</key>
                                        <true/>
                                        <key>FileVault</key>
                                        <false/>
                                        <key>Locked</key>
                                        <false/>
                                        <key>Name</key>
                                        <string>edenfs:/Users/wez/fbsource/fbobjc/buck-out</string>
                                        <key>Roles</key>
                                        <array/>
                                </dict>
                        </array>
                </dict>
        </array>
</dict>
</plist>"#;
        let containers = parse_plist::<Containers>(data).unwrap().containers;
        assert_eq!(
            containers,
            vec![ApfsContainer {
                container_reference: "disk1".to_owned(),
                volumes: vec![
                    ApfsVolume {
                        device_identifier: "disk1s1".to_owned(),
                        name: Some("Macintosh HD".to_owned()),
                    },
                    ApfsVolume {
                        device_identifier: "disk1s2".to_owned(),
                        name: Some("Preboot".to_owned()),
                    },
                    ApfsVolume {
                        device_identifier: "disk1s3".to_owned(),
                        name: Some("Recovery".to_owned()),
                    },
                    ApfsVolume {
                        device_identifier: "disk1s4".to_owned(),
                        name: Some("VM".to_owned()),
                    },
                    ApfsVolume {
                        device_identifier: "disk1s5".to_owned(),
                        name: Some("edenfs:/Users/wez/fbsource/buck-out".to_owned()),
                    },
                    ApfsVolume {
                        device_identifier: "disk1s6".to_owned(),
                        name: Some("edenfs:/Users/wez/fbsource/fbcode/buck-out".to_owned()),
                    },
                    ApfsVolume {
                        device_identifier: "disk1s7".to_owned(),
                        name: Some("edenfs:/Users/wez/fbsource/fbobjc/buck-out".to_owned()),
                    },
                ],
            },]
        );
    }

    struct FakeSystemCommand<'a> {
        default_output: Output,
        output: HashMap<&'a [&'a str], Output>,
    }

    impl<'a> FakeSystemCommand<'a> {
        fn new() -> FakeSystemCommand<'a> {
            FakeSystemCommand {
                default_output: Output {
                    status: ExitStatus::from_raw(1),
                    stdout: "".into(),
                    stderr: "Unknown command".into(),
                },
                output: HashMap::new(),
            }
        }

        fn set_output(&mut self, args: &'a [&'a str], output: Output) {
            self.output.insert(args, output);
        }

        fn set_stdout(&mut self, args: &'a [&'a str], stdout: &str) {
            self.set_output(
                args,
                Output {
                    status: ExitStatus::from_raw(0),
                    stdout: stdout.into(),
                    stderr: "".into(),
                },
            )
        }
    }

    impl SystemCommand for FakeSystemCommand<'_> {
        fn run_unprivileged(&self, args: &[&str]) -> Result<Output, std::io::Error> {
            Ok(self
                .output
                .get(args)
                .unwrap_or(&self.default_output)
                .clone())
        }
    }

    #[test]
    fn test_list_stale_volumes() {
        let mut fake_diskutil = FakeSystemCommand::new();
        let mut fake_mount = FakeSystemCommand::new();

        // An example set of APFS redirection volumes associated with an
        // ~/fbsource-dev checkout (which still exists) and an
        // ~/fbsource-removed checkout (which has been removed).
        fake_diskutil.set_stdout(
            &["apfs", "list", "-plist"],
            r#"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Containers</key>
	<array>
		<dict>
			<key>APFSContainerUUID</key>
			<string>1FD6C975-6223-4E1A-A690-4B232BBF656B</string>
			<key>CapacityCeiling</key>
			<integer>494384795648</integer>
			<key>CapacityFree</key>
			<integer>156070223872</integer>
			<key>ContainerReference</key>
			<string>disk3</string>
			<key>DesignatedPhysicalStore</key>
			<string>disk0s2</string>
			<key>Fusion</key>
			<false/>
			<key>PhysicalStores</key>
			<array>
				<dict>
					<key>DeviceIdentifier</key>
					<string>disk0s2</string>
					<key>DiskUUID</key>
					<string>01E230D7-335F-43DE-89AB-9D12541BBF9C</string>
					<key>Size</key>
					<integer>494384795648</integer>
				</dict>
			</array>
			<key>Volumes</key>
			<array>
				<dict>
					<key>APFSVolumeUUID</key>
					<string>B27A32F3-A436-4035-9860-23224584D989</string>
					<key>CapacityInUse</key>
					<integer>24576</integer>
					<key>CapacityQuota</key>
					<integer>0</integer>
					<key>CapacityReserve</key>
					<integer>0</integer>
					<key>CryptoMigrationOn</key>
					<false/>
					<key>DeviceIdentifier</key>
					<string>disk3s20</string>
					<key>Encryption</key>
					<true/>
					<key>FileVault</key>
					<false/>
					<key>Locked</key>
					<false/>
					<key>Name</key>
					<string>edenfs:/Users/mshroyer/fbsource-dev/scripts/mshroyer/bind-mounted</string>
					<key>Roles</key>
					<array/>
				</dict>
				<dict>
					<key>APFSVolumeUUID</key>
					<string>9FF5842B-9F0E-4973-9B00-BF8F0D83378A</string>
					<key>CapacityInUse</key>
					<integer>24576</integer>
					<key>CapacityQuota</key>
					<integer>0</integer>
					<key>CapacityReserve</key>
					<integer>0</integer>
					<key>CryptoMigrationOn</key>
					<false/>
					<key>DeviceIdentifier</key>
					<string>disk3s21</string>
					<key>Encryption</key>
					<true/>
					<key>FileVault</key>
					<false/>
					<key>Locked</key>
					<false/>
					<key>Name</key>
					<string>edenfs:/Users/mshroyer/fbsource-dev/scripts/mshroyer/bind-unmounted</string>
					<key>Roles</key>
					<array/>
				</dict>
				<dict>
					<key>APFSVolumeUUID</key>
					<string>BF78A4AD-FCD1-4A94-937C-A56AD6501DC0</string>
					<key>CapacityInUse</key>
					<integer>24576</integer>
					<key>CapacityQuota</key>
					<integer>0</integer>
					<key>CapacityReserve</key>
					<integer>0</integer>
					<key>CryptoMigrationOn</key>
					<false/>
					<key>DeviceIdentifier</key>
					<string>disk3s18</string>
					<key>Encryption</key>
					<true/>
					<key>FileVault</key>
					<false/>
					<key>Locked</key>
					<false/>
					<key>Name</key>
					<string>edenfs:1787fb18a9dbaea58de431bd75a7a5f0f740247468a21c615f340e8d89b1034f</string>
					<key>Roles</key>
					<array/>
				</dict>
				<dict>
					<key>APFSVolumeUUID</key>
					<string>9DDD6587-75CF-4F11-B49E-49859C7B3F52</string>
					<key>CapacityInUse</key>
					<integer>24576</integer>
					<key>CapacityQuota</key>
					<integer>0</integer>
					<key>CapacityReserve</key>
					<integer>0</integer>
					<key>CryptoMigrationOn</key>
					<false/>
					<key>DeviceIdentifier</key>
					<string>disk3s19</string>
					<key>Encryption</key>
					<true/>
					<key>FileVault</key>
					<false/>
					<key>Locked</key>
					<false/>
					<key>Name</key>
					<string>edenfs:54aca1e62d37d4b8c3de88b8fe2edcb3ab6d20887fabc5f5bf6735c526400c95</string>
					<key>Roles</key>
					<array/>
				</dict>
				<dict>
					<key>APFSVolumeUUID</key>
					<string>42BCA291-C206-4BE2-BA92-30669FDF8378</string>
					<key>CapacityInUse</key>
					<integer>24576</integer>
					<key>CapacityQuota</key>
					<integer>0</integer>
					<key>CapacityReserve</key>
					<integer>0</integer>
					<key>CryptoMigrationOn</key>
					<false/>
					<key>DeviceIdentifier</key>
					<string>disk3s22</string>
					<key>Encryption</key>
					<true/>
					<key>FileVault</key>
					<false/>
					<key>Locked</key>
					<false/>
					<key>Name</key>
					<string>edenfs:ea3773e86afbd3ede4b97ecdb8293fbaebe198143a28aa66347d81a812cd0148</string>
					<key>Roles</key>
					<array/>
				</dict>
				<dict>
					<key>APFSVolumeUUID</key>
					<string>D23A1469-A207-477E-90D4-06771F9F0FB0</string>
					<key>CapacityInUse</key>
					<integer>24576</integer>
					<key>CapacityQuota</key>
					<integer>0</integer>
					<key>CapacityReserve</key>
					<integer>0</integer>
					<key>CryptoMigrationOn</key>
					<false/>
					<key>DeviceIdentifier</key>
					<string>disk3s23</string>
					<key>Encryption</key>
					<true/>
					<key>FileVault</key>
					<false/>
					<key>Locked</key>
					<false/>
					<key>Name</key>
					<string>edenfs:b6e8f7353dea3aef8f3c717acb1b590f1fefe7b24bcc3dd3d840458eafc0a0a4</string>
					<key>Roles</key>
					<array/>
				</dict>
				<dict>
					<key>APFSVolumeUUID</key>
					<string>BC860883-7623-4EAE-8677-E362997275BE</string>
					<key>CapacityInUse</key>
					<integer>24576</integer>
					<key>CapacityQuota</key>
					<integer>0</integer>
					<key>CapacityReserve</key>
					<integer>0</integer>
					<key>CryptoMigrationOn</key>
					<false/>
					<key>DeviceIdentifier</key>
					<string>disk3s24</string>
					<key>Encryption</key>
					<true/>
					<key>FileVault</key>
					<false/>
					<key>Locked</key>
					<false/>
					<key>Name</key>
					<string>edenfs:/Users/mshroyer/fbsource-removed/scripts/mshroyer/bind-mounted</string>
					<key>Roles</key>
					<array/>
				</dict>
				<dict>
					<key>APFSVolumeUUID</key>
					<string>E3C2DE84-DA6A-4523-9FCA-93AA92E00AA6</string>
					<key>CapacityInUse</key>
					<integer>24576</integer>
					<key>CapacityQuota</key>
					<integer>0</integer>
					<key>CapacityReserve</key>
					<integer>0</integer>
					<key>CryptoMigrationOn</key>
					<false/>
					<key>DeviceIdentifier</key>
					<string>disk3s25</string>
					<key>Encryption</key>
					<true/>
					<key>FileVault</key>
					<false/>
					<key>Locked</key>
					<false/>
					<key>Name</key>
					<string>edenfs:/Users/mshroyer/fbsource-removed/scripts/mshroyer/bind-unmounted</string>
					<key>Roles</key>
					<array/>
				</dict>
			</array>
		</dict>
	</array>
</dict>
</plist>
"#);

        // The *-mounted* redirections are currently mounted.
        fake_mount.set_stdout(&[], r#"
/dev/disk3s20 on /Users/mshroyer/fbsource-dev/scripts/mshroyer/bind-mounted (apfs, local, nodev, nosuid, journaled, nobrowse, protect)
/dev/disk3s18 on /Users/mshroyer/fbsource-dev/scripts/mshroyer/bind-mounted-withsomeabsurdlylongpathname-LoremipsumdolorsitametconsecteturadipiscingelitseddoeiusmodtemporincididuntutlaboreetdoloremagnaaliquaUtenimadminimveniamquisnostrudexercitationullamc (apfs, local, nodev, nosuid, journaled, nobrowse, protect)
/dev/disk3s24 on /Users/mshroyer/fbsource-removed/scripts/mshroyer/bind-mounted (apfs, local, nodev, nosuid, journaled, nobrowse, protect)
/dev/disk3s22 on /Users/mshroyer/fbsource-removed/scripts/mshroyer/bind-mounted-withsomeabsurdlylongpathname-LoremipsumdolorsitametconsecteturadipiscingelitseddoeiusmodtemporincididuntutlaboreetdoloremagnaaliquaUtenimadminimveniamquisnostrudexercitationullamc (apfs, local, nodev, nosuid, journaled, nobrowse, protect)
        "#);

        // The ~/fbsource-removed checkout was removed, so only redirections in
        // ~/fbsource-dev are still configured.
        let configured_redirection_mount_points = vec![
            PathBuf::from("/Users/mshroyer/fbsource-dev/scripts/mshroyer/bind-mounted"),
            PathBuf::from("/Users/mshroyer/fbsource-dev/scripts/mshroyer/bind-unmounted"),
            PathBuf::from(
                "/Users/mshroyer/fbsource-dev/scripts/mshroyer/bind-mounted-withsomeabsurdlylongpathname-LoremipsumdolorsitametconsecteturadipiscingelitseddoeiusmodtemporincididuntutlaboreetdoloremagnaaliquaUtenimadminimveniamquisnostrudexercitationullamc",
            ),
            PathBuf::from(
                "/Users/mshroyer/fbsource-dev/scripts/mshroyer/bind-unmounted-withsomeabsurdlylongpathname-LoremipsumdolorsitametconsecteturadipiscingelitseddoeiusmodtemporincididuntutlaboreetdoloremagnaaliquaUtenimadminimveniamquisnostrudexercitationullamc",
            ),
        ];

        let apfs_util = ApfsUtil {
            diskutil: fake_diskutil,
            mount: fake_mount,
        };

        let stale_volumes = apfs_util.list_stale_volumes(&configured_redirection_mount_points);
        assert!(stale_volumes.is_ok());

        let mut stale_volume_names = HashSet::new();
        for vol in stale_volumes.unwrap() {
            stale_volume_names.insert(vol.name.unwrap_or("<unnamed volume>".to_string()));
        }

        // Only volumes created for currently unmounted and unconfigured
        // redirections should be considered "stale".
        assert_eq!(
            stale_volume_names,
            HashSet::from([
                "edenfs:/Users/mshroyer/fbsource-removed/scripts/mshroyer/bind-unmounted"
                    .to_string(),
                "edenfs:b6e8f7353dea3aef8f3c717acb1b590f1fefe7b24bcc3dd3d840458eafc0a0a4"
                    .to_string(),
            ])
        );
    }
}
