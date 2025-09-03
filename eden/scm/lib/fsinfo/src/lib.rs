/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;
use std::io;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;

#[cfg(target_os = "freebsd")]
use self::freebsd::fstype as fstype_imp;
#[cfg(target_os = "linux")]
use self::linux::fstype as fstype_imp;
#[cfg(target_os = "macos")]
use self::macos::fstype as fstype_imp;
#[cfg(windows)]
use self::windows::fstype as fstype_imp;

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub enum FsType {
    EDENFS,
    NTFS,
    APFS,
    HFS,
    EXT4,
    BTRFS,
    XFS,
    UFS,
    ZFS,
    NFS,
    FUSE,
    TMPFS,
    /// The catch-all type for the unknown filesystems. The content of the string is as returned
    /// from the OS and cannot be relied upon.
    Unknown(String),
}

impl fmt::Display for FsType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsType::EDENFS => write!(f, "EdenFS"),
            FsType::NTFS => write!(f, "NTFS"),
            FsType::APFS => write!(f, "APFS"),
            FsType::HFS => write!(f, "HFS"),
            FsType::EXT4 => write!(f, "ext4"),
            FsType::BTRFS => write!(f, "Btrfs"),
            FsType::XFS => write!(f, "XFS"),
            FsType::UFS => write!(f, "UFS"),
            FsType::ZFS => write!(f, "ZFS"),
            FsType::NFS => write!(f, "NFS"),
            FsType::FUSE => write!(f, "FUSE"),
            FsType::TMPFS => write!(f, "tmpfs"),
            FsType::Unknown(fstype) => write!(f, "Unknown({})", fstype),
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::ptr::null_mut;

    use winapi::shared::minwindef::DWORD;
    use winapi::shared::minwindef::MAX_PATH;
    use winapi::shared::minwindef::ULONG;
    use winapi::shared::winerror::ERROR_NOT_A_REPARSE_POINT;
    use winapi::um::fileapi::CreateFileW;
    use winapi::um::fileapi::GetVolumeInformationByHandleW;
    use winapi::um::fileapi::OPEN_EXISTING;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::ioapiset::DeviceIoControl;
    use winapi::um::winbase::FILE_FLAG_BACKUP_SEMANTICS;
    use winapi::um::winioctl::FSCTL_GET_REPARSE_POINT;
    use winapi::um::winnt::FILE_GENERIC_READ;
    use winapi::um::winnt::FILE_SHARE_DELETE;
    use winapi::um::winnt::FILE_SHARE_READ;
    use winapi::um::winnt::FILE_SHARE_WRITE;
    use winapi::um::winnt::HANDLE;
    use winapi::um::winnt::IO_REPARSE_TAG_GVFS;
    use winapi::um::winnt::MAXIMUM_REPARSE_DATA_BUFFER_SIZE;
    use winapi::um::winnt::REPARSE_GUID_DATA_BUFFER;

    use super::*;

    struct WinFileHandle {
        handle: HANDLE,
    }

    impl Drop for WinFileHandle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.handle) };
        }
    }

    fn reparse_tag(handle: &WinFileHandle) -> Result<ULONG> {
        let mut data_buffer = [0u8; MAXIMUM_REPARSE_DATA_BUFFER_SIZE as usize];

        let mut bytes_written = 0;

        let success = unsafe {
            DeviceIoControl(
                handle.handle,
                FSCTL_GET_REPARSE_POINT,
                null_mut(),
                0,
                data_buffer.as_mut_ptr() as *mut _,
                data_buffer.len() as DWORD,
                &mut bytes_written,
                null_mut(),
            )
        };

        if success == 0 {
            let err = io::Error::last_os_error();

            // In theory we should be testing if the file has a reparse point attached to it first,
            // but it looks like files/directories in ProjectedFS appear as just plain regular
            // files, thus try to get the reparse point and bail if it isn't one instead of
            // failing.
            if err.raw_os_error() == Some(ERROR_NOT_A_REPARSE_POINT as i32) {
                Ok(0)
            } else {
                Err(err.into())
            }
        } else {
            let reparse_buffer =
                unsafe { &*(data_buffer.as_ptr() as *const REPARSE_GUID_DATA_BUFFER) };
            Ok(reparse_buffer.ReparseTag)
        }
    }

    fn is_edenfs(handle: &WinFileHandle) -> Result<bool> {
        let tag = reparse_tag(handle)?;
        Ok(tag == IO_REPARSE_TAG_GVFS)
    }

    fn open_share(path: &Path) -> Result<WinFileHandle> {
        let mut root: Vec<u16> = path.as_os_str().encode_wide().collect();
        // Need to make it 0 terminated,
        // otherwise might not get the correct
        // string
        root.push(0);

        let handle = unsafe {
            CreateFileW(
                root.as_mut_ptr(),
                FILE_GENERIC_READ,
                FILE_SHARE_DELETE | FILE_SHARE_READ | FILE_SHARE_WRITE,
                null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                null_mut(),
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error().into())
        } else {
            Ok(WinFileHandle { handle })
        }
    }

    fn edenfs_directory(mut path: &Path) -> Option<PathBuf> {
        if path.is_file() {
            // Safe to unwrap as a file always has a parent directory.
            path = path.parent().unwrap();
        }
        loop {
            if identity::must_sniff_dir(path).is_ok() {
                // The .eden directory is at the root of the repo, don't recurse further if it's
                // not there.
                let dot_eden = path.join(".eden");
                if dot_eden.exists() {
                    return Some(dot_eden);
                } else {
                    return None;
                }
            }

            path = match path.parent() {
                None => return None,
                Some(path) => path,
            }
        }
    }

    impl From<String> for FsType {
        fn from(value: String) -> Self {
            match value.as_ref() {
                "NTFS" => FsType::NTFS,
                _ => FsType::Unknown(value),
            }
        }
    }

    pub fn fstype(path: &Path) -> Result<FsType> {
        let win_handle = match edenfs_directory(path) {
            None => open_share(path)?,
            Some(dot_eden) => open_share(&dot_eden)?,
        };

        if is_edenfs(&win_handle)? {
            return Ok(FsType::EDENFS);
        }

        let mut fstype = [0u16; MAX_PATH];
        let exit_sts = unsafe {
            GetVolumeInformationByHandleW(
                win_handle.handle,
                null_mut(),
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                fstype.as_mut_ptr(),
                fstype.len() as DWORD,
            )
        };

        if exit_sts == 0 {
            return Err(io::Error::last_os_error().into());
        }
        // Take until the first 0 byte
        let terminator = fstype.iter().position(|&x| x == 0).unwrap();
        let fstype = String::from_utf16(&fstype[0..terminator])?;

        Ok(fstype.into())
    }
}

#[cfg(unix)]
mod unix {
    use std::ffi::CString;
    use std::mem::zeroed;
    use std::os::unix::ffi::OsStrExt;

    use super::*;

    pub fn get_statfs(path: &Path) -> Result<libc::statfs> {
        let cstr = CString::new(path.as_os_str().as_bytes())?;
        let mut fs_stat: libc::statfs = unsafe { zeroed() };
        if unsafe { libc::statfs(cstr.as_ptr(), &mut fs_stat) } == 0 {
            Ok(fs_stat)
        } else {
            Err(io::Error::last_os_error().into())
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::os::linux::fs::MetadataExt;

    use super::*;

    /// These filesystem types are not in libc yet
    const BTRFS_SUPER_MAGIC: i64 = 0x9123683e;
    const FUSE_SUPER_MAGIC: i64 = 0x65735546;
    const XFS_SUPER_MAGIC: i64 = 0x58465342;

    impl From<i64> for FsType {
        fn from(f_type: i64) -> Self {
            match f_type {
                BTRFS_SUPER_MAGIC => FsType::BTRFS,
                FUSE_SUPER_MAGIC => FsType::FUSE,
                XFS_SUPER_MAGIC => FsType::XFS,
                libc::EXT4_SUPER_MAGIC => FsType::EXT4,
                libc::NFS_SUPER_MAGIC => FsType::NFS,
                libc::TMPFS_MAGIC => FsType::TMPFS,
                libc::CODA_SUPER_MAGIC => FsType::Unknown("coda".to_string()),
                libc::CRAMFS_MAGIC => FsType::Unknown("cramfs".to_string()),
                libc::EFS_SUPER_MAGIC => FsType::Unknown("efs".to_string()),
                libc::HPFS_SUPER_MAGIC => FsType::Unknown("hpfs".to_string()),
                libc::HUGETLBFS_MAGIC => FsType::Unknown("hugetlbfs".to_string()),
                libc::ISOFS_SUPER_MAGIC => FsType::Unknown("isofs".to_string()),
                libc::JFFS2_SUPER_MAGIC => FsType::Unknown("jffs2".to_string()),
                libc::MINIX2_SUPER_MAGIC | libc::MINIX2_SUPER_MAGIC2 => {
                    FsType::Unknown("minix2".to_string())
                }
                libc::MINIX_SUPER_MAGIC | libc::MINIX_SUPER_MAGIC2 => {
                    FsType::Unknown("minix".to_string())
                }
                libc::NCP_SUPER_MAGIC => FsType::Unknown("ncp".to_string()),
                libc::OPENPROM_SUPER_MAGIC => FsType::Unknown("openprom".to_string()),
                libc::PROC_SUPER_MAGIC => FsType::Unknown("proc".to_string()),
                libc::QNX4_SUPER_MAGIC => FsType::Unknown("qnx4".to_string()),
                libc::REISERFS_SUPER_MAGIC => FsType::Unknown("reiserfs".to_string()),
                libc::SMB_SUPER_MAGIC => FsType::Unknown("smb".to_string()),
                libc::USBDEVICE_SUPER_MAGIC => FsType::Unknown("usbdevice".to_string()),
                _ => FsType::Unknown(format!("{:#X}", f_type)),
            }
        }
    }

    fn get_type(f_type: i64, path: &Path) -> Result<FsType> {
        let result = FsType::from(f_type);
        if result == FsType::FUSE {
            // .eden is present in all directories in an EdenFS mount.
            if path.join(".eden").exists() {
                return Ok(FsType::EDENFS);
            } else {
                // Take some efforts to find out the actual filesystem.
                // This works for Linux block devices.
                if let Some(major_minor) = get_dev_major_minor(path) {
                    let props = find_udev_properties(&major_minor);
                    if let Some(name) = props.get("E:ID_FS_TYPE") {
                        if name == "ntfs" {
                            return Ok(FsType::NTFS);
                        } else {
                            return Ok(FsType::Unknown(format!("fuse.{}", name)));
                        }
                    }
                }
            }
        }
        Ok(result)
    }

    /// Find the udev properties for the block device. Best-effort.
    fn find_udev_properties(major_minor: &str) -> HashMap<String, String> {
        // The path is found by stracing `blkid`.
        // To do this "properly", consider https://docs.rs/udev/0.3.0/udev/struct.Device.html
        let path = format!("/run/udev/data/b{}", major_minor);
        let mut result = HashMap::new();
        for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
            if line.contains('=') {
                let words: Vec<_> = line.splitn(2, '=').collect();
                result.insert(words[0].into(), words[1].into());
            }
        }
        result
    }

    /// Get the "st_dev". Return the major:minor form (ex. "8:1").
    fn get_dev_major_minor(path: &Path) -> Option<String> {
        path.symlink_metadata().ok().map(|m| {
            let st_dev = m.st_dev();
            let (major, minor) = (libc::major(st_dev), libc::minor(st_dev));
            format!("{}:{}", major, minor)
        })
    }

    pub fn fstype(path: &Path) -> Result<FsType> {
        let fs_stat = super::unix::get_statfs(path)?;
        get_type(fs_stat.f_type, path)
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CStr;

    use super::*;

    impl<'a> From<&'a str> for FsType {
        fn from(value: &'a str) -> Self {
            match value {
                "apfs" => FsType::APFS,
                "hfs" => FsType::HFS,
                "edenfs_eden" => FsType::EDENFS,
                "macfuse_eden" => FsType::EDENFS,
                "osxfuse_eden" => FsType::EDENFS,
                "edenfs:" => FsType::EDENFS,
                _ => FsType::Unknown(value.to_string()),
            }
        }
    }

    pub fn fstype(path: &Path) -> Result<FsType> {
        let fs_stat = super::unix::get_statfs(path)?;
        let fs = unsafe { CStr::from_ptr(fs_stat.f_fstypename.as_ptr()) };

        Ok(fs.to_str()?.into())
    }
}

#[cfg(target_os = "freebsd")]
mod freebsd {
    use std::ffi::CStr;

    use super::*;

    impl<'a> From<&'a str> for FsType {
        fn from(value: &'a str) -> Self {
            match value {
                "ufs" => FsType::UFS,
                "zfs" => FsType::ZFS,
                _ => FsType::Unknown(value.to_string()),
            }
        }
    }

    pub fn fstype(path: &Path) -> Result<FsType> {
        let fs_stat = super::unix::get_statfs(path)?;
        let fs = unsafe { CStr::from_ptr(fs_stat.f_fstypename.as_ptr()) };

        Ok(fs.to_str()?.into())
    }
}

/// Get filesystem type on the given `path`.
pub fn fstype(path: impl AsRef<Path>) -> Result<FsType> {
    let path = path.as_ref();

    // Auto correct an empty path to ".".
    let path = if path == Path::new("") {
        Path::new(".")
    } else {
        path
    };

    fstype_imp(path).with_context(|| format!("Cannot determine filesystem type for {:?}", path))
}
