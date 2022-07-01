/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use regex::Regex;
use std::path::PathBuf;
use subprocess::Exec;
use subprocess::Redirection;

#[derive(Debug, PartialEq)]
pub(crate) struct MountTableInfo {
    device: String,
    mount_point: PathBuf,
    vfstype: String,
}

impl MountTableInfo {
    pub(crate) fn mount_point(&self) -> PathBuf {
        self.mount_point.clone()
    }
}

fn parse_linux_mtab(mtab_string: String) -> Vec<MountTableInfo> {
    let mut mounts = Vec::new();
    for line in mtab_string.trim().lines() {
        let entries: Vec<&str> = line.split_ascii_whitespace().collect();
        if entries.len() != 6 {
            eprintln!(
                "mount table line `{}` has {} entries instead of 6",
                line,
                entries.len()
            );
        } else if let [device, mount_point, vfstype, _opts, _freq, _passno] = &entries[..] {
            mounts.push(MountTableInfo {
                device: String::from(*device),
                mount_point: PathBuf::from(*mount_point),
                vfstype: String::from(*vfstype),
            });
        }
    }
    mounts
}

fn parse_macos_mtab(mtab_string: String) -> Vec<MountTableInfo> {
    let mut mounts = Vec::new();
    let mount_regex = Regex::new(r"^(\S+) on (.+) \(([^,]+),.*\)$")
        .expect("Expect each macos mtab to follow a specific format.");
    for line in mtab_string.split('\n') {
        for caps in mount_regex.captures_iter(line) {
            mounts.push(MountTableInfo {
                device: String::from(&caps[1]),
                mount_point: PathBuf::from(&caps[2]),
                vfstype: String::from(&caps[3]),
            });
        }
    }
    mounts
}

/// Returns the list of system mounts
pub(crate) fn read_mount_table() -> Result<Vec<MountTableInfo>> {
    if cfg!(target_os = "linux") {
        Ok(parse_linux_mtab(
            std::fs::read_to_string(PathBuf::from("/proc/self/mounts")).from_err()?,
        ))
    } else if cfg!(target_os = "macos") {
        // Specifying the path is important, as sudo may have munged the path
        // such that /sbin is not part of it any longer
        let output = Exec::cmd("/sbin/mount")
            .stdout(Redirection::Pipe)
            .stderr(Redirection::Pipe)
            .capture()
            .from_err()?;

        if output.success() {
            Ok(parse_macos_mtab(output.stdout_str()))
        } else {
            Err(EdenFsError::Other(anyhow!(
                "Failed to execute /sbin/mount, stderr: {}",
                output.stderr_str()
            )))
        }
    } else {
        Ok(Vec::new())
    }
}

#[test]
fn test_parse_linux_mtab() {
    let contents = "
homedir.eden.com:/home109/chadaustin/public_html /mnt/public/chadaustin nfs rw,context=user_u:object_r:user_home_dir_t,relatime,vers=3,rsize=65536,wsize=65536,namlen=255,soft,nosharecache,proto=tcp6,timeo=100,retrans=2,sec=krb5i,mountaddr=2401:db00:fffe:1007:face:0000:0:4007,mountvers=3,mountport=635,mountproto=udp6,local_lock=none,addr=2401:db00:fffe:1007:0000:b00c:0:4007 0 0
squashfuse_ll /mnt/xarfuse/uid-0/2c071047-ns-4026531840 fuse.squashfuse_ll rw,nosuid,nodev,relatime,user_id=0,group_id=0 0 0
bogus line here
edenfs: /tmp/eden_test.4rec6drf/mounts/main fuse rw,nosuid,relatime,user_id=138655,group_id=100,default_permissions,allow_other 0 0
".to_string();
    let mount_infos = parse_linux_mtab(contents);
    assert_eq!(3, mount_infos.len());
    assert_eq!("edenfs:", mount_infos[2].device);
    assert_eq!(
        PathBuf::from("/tmp/eden_test.4rec6drf/mounts/main"),
        mount_infos[2].mount_point
    );
    assert_eq!("fuse", mount_infos[2].vfstype);
}

#[test]
fn test_parse_mtab_macos() {
    let contents = "
/dev/disk1s1 on / (apfs, local, journaled)
devfs on /dev (devfs, local, nobrowse)
/dev/disk1s4 on /private/var/vm (apfs, local, noexec, journaled, noatime, nobrowse)
map -hosts on /net (autofs, nosuid, automounted, nobrowse)
map auto_home on /home (autofs, automounted, nobrowse)
map -fstab on /Network/Servers (autofs, automounted, nobrowse)
eden@osxfuse0 on /Users/wez/fbsource (osxfuse_eden, nosuid, synchronous)
"
    .to_string();

    let expected = vec![
        MountTableInfo {
            device: "/dev/disk1s1".to_string(),
            mount_point: PathBuf::from("/"),
            vfstype: "apfs".to_string(),
        },
        MountTableInfo {
            device: "devfs".to_string(),
            mount_point: PathBuf::from("/dev"),
            vfstype: "devfs".to_string(),
        },
        MountTableInfo {
            device: "/dev/disk1s4".to_string(),
            mount_point: PathBuf::from("/private/var/vm"),
            vfstype: "apfs".to_string(),
        },
        MountTableInfo {
            device: "eden@osxfuse0".to_string(),
            mount_point: PathBuf::from("/Users/wez/fbsource"),
            vfstype: "osxfuse_eden".to_string(),
        },
    ];
    let actual = parse_macos_mtab(contents);

    assert_eq!(expected.len(), actual.len());
    assert!(expected.iter().zip(&actual).all(|(a, b)| *a == *b));
}
