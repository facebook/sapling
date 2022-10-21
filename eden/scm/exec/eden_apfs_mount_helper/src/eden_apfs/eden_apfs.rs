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
use std::process::Output;
use std::str;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
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

    pub fn list_stale_volumes(&self, all_checkouts: &[String]) -> Result<Vec<ApfsVolume>> {
        let all_checkouts = all_checkouts
            .iter()
            .map(|v| canonicalize_mount_point_path(v.as_ref()))
            .collect::<Result<Vec<_>>>()?;
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

                let is_stale = all_checkouts
                    .iter()
                    .try_fold(false, |acc, checkout| {
                        vol.is_preferred_checkout(checkout).map(|p| acc || p)
                    })
                    .map(std::ops::Not::not)
                    .unwrap_or(false);

                if is_stale {
                    stale_volumes.push(vol);
                }
            }
        }

        Ok(stale_volumes)
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
mod test {
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
}
