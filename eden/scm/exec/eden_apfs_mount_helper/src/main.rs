/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This is a little macOS specific utility that is intended to be installed setuid root
//! so that it can mount scratch volumes into a portion of the filesytem
//! owned by a non-privileged user.
//! It is intended to be used together with edenfs, but may also be
//! useful for non-virtualized repos as a way to move IO out of a recursive
//! watch.
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::str;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use eden_apfs::*;
use once_cell::sync::Lazy;
#[cfg(target_os = "macos")]
use serde::*;
use structopt::StructOpt;

#[cfg(feature = "fb")]
mod facebook;

const MOUNT_APFS: &'static str = "/sbin/mount_apfs";
const MAX_ADDVOLUME_RETRY: u64 = 3;

static MOUNT: Lazy<SystemCommandImpl> = Lazy::new(|| SystemCommandImpl(PathBuf::from(MOUNT_PATH)));

#[derive(StructOpt, Debug)]
enum Opt {
    /// List APFS volumes
    #[structopt(name = "list")]
    List {
        #[structopt(long = "all")]
        all: bool,
    },

    /// List APFS volumes that are not mounted and not used by any of the active checkouts.
    /// The intent is that `all_checkouts` is produced by `edenfsctl list`.
    #[structopt(name = "list-stale-volumes")]
    ListStaleVolumes {
        all_checkouts: Vec<String>,
        #[structopt(long = "json")]
        json: bool,
    },

    /// Mount some space at the specified path.
    /// You must be the owner of the path.
    #[structopt(name = "mount")]
    Mount { mount_point: String },

    /// Unmount the eden space from a specific path.
    /// This will only allow unmounting volumes that were created
    /// by this utility.
    #[structopt(name = "unmount")]
    UnMount {
        /// The mounted path that you wish to unmount
        mount_point: String,
        /// Force the unmount, even if files are open and busy
        #[structopt(long = "force")]
        force: bool,
    },

    /// Unmount and delete a volume associated with a specific path.
    /// This will only allow deleting volumes that were created
    /// by this utility
    #[structopt(name = "delete")]
    Delete {
        /// The mounted path that you wish to unmount
        mount_point: String,
    },

    /// Unmount and delete all APFS volumes created by this utility
    #[structopt(name = "delete-all")]
    DeleteAll {
        #[structopt(long = "kill_dependent_processes")]
        kill_dependent_processes: bool,
    },

    /// Unmount and delete a volume.
    /// This will only allow deleting volumes that were created
    /// by this utility
    #[structopt(name = "delete-volume")]
    DeleteVolume {
        /// The volume that you wish to delete
        volume: String,
    },
}

#[cfg(target_os = "macos")]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PartitionInfo {
    parent_whole_disk: String,
}

fn kill_active_pids_in_mounts(mut mount_points: Vec<String>) -> Result<()> {
    if mount_points.is_empty() {
        println!("Not killing anything as there are no mounts");
        return Ok(());
    }

    mount_points.push("-t".to_owned());
    // -t has to come before the mount point list
    let last_index = mount_points.len() - 1;
    mount_points.swap(0, last_index);
    let lsof: &str = "/usr/sbin/lsof";
    println!(
        "Listing dependent processes with: `{} {}`",
        lsof,
        mount_points.join(" ")
    );
    let mut active_pids = new_cmd_with_best_available_privs(lsof)
        .args(&mount_points)
        .stdout(Stdio::piped())
        .spawn()?;
    let active_pids_output = (active_pids.stdout.take()).context("lsof stdout not available")?;

    let xargs = "/usr/bin/xargs";
    let kill_args: Vec<&str> = vec!["-t", "/bin/kill", "-9"];
    println!(
        "and then killing them with : `{} {}`",
        xargs,
        kill_args.join(" ")
    );

    let output = new_cmd_with_best_available_privs(xargs)
        .args(&kill_args)
        .stdin(active_pids_output)
        .output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "failed to execute lsof {} | xargs kill -9 \n {:#?}",
            mount_points.join(" "),
            output
        ));
    }
    println!("result: {:?}", output);
    Ok(())
}

fn list_mount_points(containers: &Vec<ApfsContainer>, mounts: &MountTable) -> Result<Vec<String>> {
    let mut mount_points: Vec<String> = vec![];

    for container in containers {
        for vol in &container.volumes {
            if vol.is_edenfs_managed_volume() {
                if let Some(mount_point) = vol.get_current_mount_point(&*MOUNT, Some(mounts)) {
                    mount_points.push(mount_point);
                }
            }
        }
    }

    Ok(mount_points)
}

/// Prepare a command to be run with root privs.
/// The path must be absolute to avoid being fooled into running something
/// unexpected.
/// The caller must already have root privs, otherwise this will fail.
fn new_cmd_with_root_privs(path: &str) -> Command {
    let path: PathBuf = path.into();
    assert!(path.is_absolute());
    assert!(
        geteuid() == 0,
        "root privs are required to run {}",
        path.display()
    );
    Command::new(path)
}

/// Prepare a command that will be run as root if we have root privileges, and
/// unprivileged if we do not. Use sparingly, running as root or non root is
/// better if it suits your needs.
/// This should be used for commands that can run both privileged and
/// unprivileged. Where privileged where privilege may give them more
/// capabilities, but they are best effort any ways, so we still want to try
/// to run them when we don't have privilege.
fn new_cmd_with_best_available_privs(path: &str) -> Command {
    let path: PathBuf = path.into();
    assert!(path.is_absolute());
    Command::new(path)
}

/// Create a new subvolume with the specified name.
/// Note that this does NOT require any special privilege on macOS.
///
/// Note that this code tries to create the subvolume multiple times to workaround a bug where the
/// `diskutil apfs addVolume` command succeeds but the subvolume isn't created. Apple claims that
/// this is fixed in macOS 11.5 but Sandcastle isn't on 11.5 yet.
fn make_new_volume(apfs_util: &ApfsUtil, name: &str, disk: &str) -> Result<ApfsVolume> {
    let mut tried = 0;
    loop {
        let output = new_cmd_unprivileged(DISKUTIL_PATH)
            .args(&["apfs", "addVolume", disk, "apfs", name, "-nomount"])
            .output()?;
        if !output.status.success() {
            anyhow::bail!("failed to execute diskutil addVolume: {:?}", output);
        }
        let containers = apfs_util.list_containers()?;

        if let Some(volume) = find_existing_volume(&containers, name) {
            return Ok(volume.clone());
        } else {
            tried += 1;
            if tried == MAX_ADDVOLUME_RETRY {
                return Err(anyhow!("failed to create volume `{}`: {:#?}", name, output));
            } else {
                println!(
                    "APFS subvolume created, but not found in `diskutil apfs list`, retrying."
                );
                // Let's sleep a bit before retrying in case this is timing related.
                std::thread::sleep(Duration::from_secs(1));

                // Maybe the volume wasn't available immediately, let's see if it appeared.
                let containers = apfs_util.list_containers()?;
                if let Some(volume) = find_existing_volume(&containers, name) {
                    return Ok(volume.clone());
                }

                // Nope, let's just loop
            }
        }
    }
}

fn get_real_uid() -> Result<u32> {
    let uid = getuid();

    if uid != 0 {
        return Ok(uid);
    }

    // We're really root (not just setuid root).  We may actually be
    // running under sudo so let's see what sudo says about the UID
    match std::env::var("SUDO_UID") {
        Ok(uid) => Ok(uid.parse().context(format!(
            "parsing the SUDO_UID={} env var as an integer",
            uid
        ))?),
        Err(std::env::VarError::NotPresent) => Ok(uid),
        Err(std::env::VarError::NotUnicode(_)) => bail!("the SUDO_UID env var is not unicode"),
    }
}

/// Canonicalize a path and return the canonical path in string form.
#[cfg(target_os = "macos")]
fn find_disk_for_eden_mount(mount_point: &str) -> Result<String> {
    let mut client_link = PathBuf::from(mount_point);
    client_link.push(".eden");
    client_link.push("client");

    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };

    let client_link_cstr = std::ffi::CString::new(
        client_link
            .to_str()
            .ok_or_else(|| anyhow!("not a valid UTF-8 path somehow"))?,
    )?;
    let rv = unsafe { libc::statfs(client_link_cstr.as_ptr(), &mut stat) };
    if -1 == rv {
        return Err(std::io::Error::last_os_error().into());
    }

    let fstype = unsafe { std::ffi::CStr::from_ptr(stat.f_fstypename.as_ptr()).to_str()? };
    if "apfs" != fstype {
        bail!("disk at {} must be apfs", mount_point);
    }
    let partition = unsafe { std::ffi::CStr::from_ptr(stat.f_mntfromname.as_ptr()).to_str()? };
    let output = new_cmd_unprivileged(DISKUTIL_PATH)
        .args(&["info", "-plist", &partition])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("failed to execute diskutil info: {:?}", output);
    }

    Ok(parse_plist::<PartitionInfo>(&String::from_utf8(output.stdout)?)?.parent_whole_disk)
}

#[cfg(not(target_os = "macos"))]
fn find_disk_for_eden_mount(_mount_point: &str) -> Result<String> {
    Err(anyhow!("only supported on macOS"))
}

fn mount_scratch_space_on(apfs_util: &ApfsUtil, input_mount_point: &str) -> Result<()> {
    let mount_point = canonicalize_mount_point_path(input_mount_point)?;
    println!("want to mount at {:?}", mount_point);

    // First, let's ensure that mounting at this location makes sense.
    // Inspect the directory and ensure that it is owned by us.
    let metadata = std::fs::metadata(&mount_point)
        .context(format!("Obtaining filesystem metadata for {}", mount_point))?;
    let my_uid = get_real_uid()?;
    if metadata.uid() != my_uid {
        bail!(
            "Refusing to set up a volume for {} because the owned uid {} doesn't match your uid {}",
            mount_point,
            metadata.uid(),
            my_uid
        );
    }

    println!("my real uid is {}, effective is {}", my_uid, unsafe {
        libc::geteuid()
    });

    let containers = apfs_util.list_containers()?;
    let name = encode_mount_point_as_volume_name(&mount_point);
    let volume = match find_existing_volume(&containers, &name) {
        Some(existing) => {
            let mount_table = MountTable::parse_system_mount_table(&*MOUNT)?;
            if let Some(current_mount_point) =
                existing.get_current_mount_point(&*MOUNT, Some(&mount_table))
            {
                if !existing.is_preferred_location(&current_mount_point)? {
                    // macOS will automatically mount volumes at system boot,
                    // but mount them under /Volumes.  That will block our attempt
                    // to mount the scratch space below, so if we see that this
                    // volume is mounted and not where we want it, we simply unmount
                    // it here now: this should be fine because we own these volumes
                    // and where they get mounted.  No one else should have a legit
                    // reason for mounting it elsewhere.
                    apfs_util.unmount_scratch(&mount_point, true, &mount_table)?;
                }
            }
            existing.clone()
        }
        None => make_new_volume(apfs_util, &name, &find_disk_for_eden_mount(&mount_point)?)?,
    };

    // Mount the volume at the desired mount point.
    // This is the only part of this utility that requires root privs.
    let output = new_cmd_with_root_privs(MOUNT_APFS)
        .args(&[
            "-onobrowse,nodev,nosuid",
            "-u",
            &format!("{}", metadata.uid()),
            "-g",
            &format!("{}", metadata.gid()),
            &format!("/dev/{}", volume.device_identifier),
            &mount_point,
        ])
        .output()?;
    if !output.status.success() {
        // See [`crate::facebook::write_apfs_issue_marker`] for detail
        #[cfg(feature = "fb")]
        if let Some(code) = output.status.code() {
            // This is the error code we get when we failed to mount the APFS subvolumes.
            if code == 66 {
                crate::facebook::write_apfs_issue_marker();
            }
        }
        anyhow::bail!(
            "failed to execute mount_apfs /dev/{} {}: {:#?}",
            volume.device_identifier,
            mount_point,
            output
        );
    }
    println!("output: {:?}", output);

    // Make sure that we own the mounted directory; the default is mounted
    // with root:wheel ownership, and that isn't desirable
    chown(&mount_point, metadata.uid(), metadata.gid())?;

    disable_spotlight(&mount_point).ok();
    disable_fsevents(&mount_point).ok();
    disable_trashcan(&mount_point).ok();

    Ok(())
}

fn chown(path: &str, uid: u32, gid: u32) -> Result<()> {
    let cstr = std::ffi::CString::new(path)
        .with_context(|| format!("creating a C string from path `{}`", path))?;
    let rc = unsafe { libc::chown(cstr.as_ptr(), uid, gid) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        Err(err).with_context(|| format!("failed to chown {} to uid={}, gid={}", path, uid, gid))
    } else {
        Ok(())
    }
}

/// Don't bother indexing an artifact dir.  It's just a waste of resources
/// to build an index for something managed entirely by the machine.
fn disable_spotlight(mount_point: &str) -> Result<()> {
    let output = new_cmd_with_root_privs("/usr/bin/mdutil")
        .args(&["-Ed", "-i", "off", mount_point])
        .output()?;
    if !output.status.success() {
        eprintln!(
            "failed to disable spotlight on {}: {:#?}",
            mount_point, output
        );
    }

    let spotlight = Path::new(mount_point).join(".Spotlight-V100");
    std::fs::remove_dir_all(&spotlight).ok();

    Ok(())
}

/// Disable fsevents logging for the artifact dirs: this is for performance
/// reasons; we don't need/want fseventsd to run here.
fn disable_fsevents(mount_point: &str) -> Result<()> {
    // See https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/FSEvents_ProgGuide/FileSystemEventSecurity/FileSystemEventSecurity.html#//apple_ref/doc/uid/TP40005289-CH6-SW5
    // Those docs say that we should recreate the directory and touch a control
    // file to disable logging data, but the presence of the directory can
    // confuse some tools, so we simply delete it at mount time; that should
    // be good enough in most cases.

    let fseventsd = Path::new(mount_point).join(".fseventsd");
    std::fs::remove_dir_all(&fseventsd).ok();

    Ok(())
}

/// The .Trashes directory has root permissions by default, which makes it
/// awkward for users to clean up the contents of the mount point when it
/// is used in place of an artifact directory.
fn disable_trashcan(mount_point: &str) -> Result<()> {
    let trashes = Path::new(mount_point).join(".Trashes");
    std::fs::remove_dir_all(trashes)?;

    // There's some thought that touching a regular file named
    // `.Trashes` is a good idea to prevent the trash dir from
    // coming back (which is what happens when using Finder to
    // send something to the trash).
    // For now we're just removing it at mount time.

    Ok(())
}

fn main() -> Result<()> {
    let opts = Opt::from_args();

    let apfs_util = ApfsUtil::new(DISKUTIL_PATH, MOUNT_PATH);

    match opts {
        Opt::List { all } => {
            let containers = apfs_util.list_containers()?;
            let mounts = MountTable::parse_system_mount_table(&*MOUNT)?;
            for container in containers {
                for vol in container.volumes {
                    if all || vol.is_edenfs_managed_volume() {
                        let name = vol.name.as_ref().map(String::as_str).unwrap_or("");
                        if let Some(mount_point) =
                            vol.get_current_mount_point(&*MOUNT, Some(&mounts))
                        {
                            println!("{}\t{}\t{}", vol.device_identifier, name, mount_point);
                        } else {
                            println!("{}\t{}", vol.device_identifier, name);
                        }
                    }
                }
            }
            Ok(())
        }

        Opt::ListStaleVolumes {
            all_checkouts,
            json,
        } => {
            let all_checkouts = all_checkouts
                .iter()
                .map(|v| canonicalize_mount_point_path(v.as_ref()))
                .collect::<Result<Vec<_>>>()?;

            let mut stale_volume_names = vec![];
            for vol in apfs_util.list_stale_volumes_unsafe(&all_checkouts)? {
                stale_volume_names.push(vol.name.context("Volume has no name")?);
            }
            if json {
                println!("{}", serde_json::to_string(&stale_volume_names)?);
            } else {
                for name in stale_volume_names.iter() {
                    println!("{}", name);
                }
            }
            Ok(())
        }

        Opt::Mount { mount_point } => mount_scratch_space_on(&apfs_util, &mount_point),

        Opt::UnMount { mount_point, force } => {
            apfs_util.unmount_scratch(
                &mount_point,
                force,
                &MountTable::parse_system_mount_table(&*MOUNT)?,
            )?;
            Ok(())
        }

        Opt::Delete { mount_point } => {
            apfs_util.delete_scratch(&mount_point)?;
            Ok(())
        }

        Opt::DeleteVolume { volume } => {
            apfs_util.delete_volume(&volume)?;
            Ok(())
        }

        Opt::DeleteAll {
            kill_dependent_processes,
        } => {
            let containers = apfs_util.list_containers()?;
            let mounts = MountTable::parse_system_mount_table(&*MOUNT)?;

            if kill_dependent_processes {
                let mount_points: Vec<String> = list_mount_points(&containers, &mounts)?;
                kill_active_pids_in_mounts(mount_points)?;
            }

            let mut was_failure = false;
            for container in containers {
                for vol in container.volumes {
                    if vol.is_edenfs_managed_volume() {
                        let mut try_delete = true;

                        if let Some(mount_point) =
                            vol.get_current_mount_point(&*MOUNT, Some(&mounts))
                        {
                            // In the context of deleting all volumes, we want to
                            // force the unmount--we know it is safe.
                            let force = true;
                            if let Err(err) =
                                apfs_util.unmount_scratch(&mount_point, force, &mounts)
                            {
                                eprintln!("Failed to unmount: {}", err);
                                try_delete = false;
                                was_failure = true;
                            }
                        }

                        if try_delete {
                            let mount_point = vol.preferred_mount_point().unwrap();
                            if let Err(err) = apfs_util.delete_scratch(&mount_point) {
                                eprintln!("Failed to delete {:#?}: {}", vol, err);
                                was_failure = true
                            } else {
                                println!("Deleted {}", mount_point);
                            }
                        }
                    }
                }
            }
            if was_failure {
                Err(anyhow!("Failed to unmount or delete one or more volumes."))
            } else {
                Ok(())
            }
        }
    }
}
