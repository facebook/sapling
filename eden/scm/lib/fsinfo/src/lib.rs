/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
use std::io;
use std::path::Path;

#[cfg(windows)]
mod windows {
    use winapi::shared::minwindef::{DWORD, MAX_PATH};
    use winapi::um::fileapi::{CreateFileW, GetVolumeInformationByHandleW, OPEN_EXISTING};
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::winnt::{
        FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, HANDLE,
    };

    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::null_mut;

    const FILE_ATTRIBUTE_NORMAL: u32 = 0x02000000;

    struct WinFileHandle {
        handle: HANDLE,
    }

    impl Drop for WinFileHandle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.handle) };
        }
    }

    fn open_share(path: &Path) -> io::Result<WinFileHandle> {
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
                FILE_ATTRIBUTE_NORMAL as DWORD,
                null_mut(),
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(WinFileHandle { handle })
        }
    }

    pub fn fstype(path: &Path) -> io::Result<String> {
        let win_handle = open_share(path)?;

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
            return Err(io::Error::last_os_error());
        }
        // Take until the first 0 byte
        let terminator = fstype.iter().position(|&x| x == 0).unwrap();
        let fstype = &fstype[0..terminator];

        Ok(String::from_utf16_lossy(&fstype))
    }
}

#[cfg(unix)]
mod unix {
    use std::ffi::CString;
    use std::io;
    use std::mem::zeroed;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    pub fn get_statfs(path: &Path) -> io::Result<libc::statfs> {
        let cstr = CString::new(path.as_os_str().as_bytes())?;
        let mut fs_stat: libc::statfs = unsafe { zeroed() };
        if unsafe { libc::statfs(cstr.as_ptr(), &mut fs_stat) } == 0 {
            Ok(fs_stat)
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io;
    use std::path::Path;

    /// These filesystem types are not in libc yet
    const BTRFS_SUPER_MAGIC: i64 = 0x9123683e;
    const CIFS_SUPER_MAGIC: i64 = 0xff534d42;
    const FUSE_SUPER_MAGIC: i64 = 0x65735546;
    const XFS_SUPER_MAGIC: i64 = 0x58465342;

    fn get_type(f_type: i64, path: &Path) -> &'static str {
        match f_type {
            BTRFS_SUPER_MAGIC => "btrfs",
            CIFS_SUPER_MAGIC => "cifs",
            FUSE_SUPER_MAGIC => {
                // .eden is present in all directories in an EdenFS mount.
                if path.join(".eden").exists() {
                    "edenfs"
                } else {
                    "fuse"
                }
            }
            XFS_SUPER_MAGIC => "xfs",
            libc::CODA_SUPER_MAGIC => "coda",
            libc::CRAMFS_MAGIC => "cramfs",
            libc::EFS_SUPER_MAGIC => "efs",
            libc::EXT4_SUPER_MAGIC => "ext4",
            libc::HPFS_SUPER_MAGIC => "hpfs",
            libc::HUGETLBFS_MAGIC => "hugetlbfs",
            libc::ISOFS_SUPER_MAGIC => "isofs",
            libc::JFFS2_SUPER_MAGIC => "jffs2",
            libc::MINIX_SUPER_MAGIC | libc::MINIX_SUPER_MAGIC2 => "minix",
            libc::MINIX2_SUPER_MAGIC | libc::MINIX2_SUPER_MAGIC2 => "minix2",
            libc::NCP_SUPER_MAGIC => "ncp",
            libc::NFS_SUPER_MAGIC => "nfs",
            libc::OPENPROM_SUPER_MAGIC => "openprom",
            libc::PROC_SUPER_MAGIC => "proc",
            libc::QNX4_SUPER_MAGIC => "qnx4",
            libc::REISERFS_SUPER_MAGIC => "reiserfs",
            libc::SMB_SUPER_MAGIC => "smb",
            libc::TMPFS_MAGIC => "tmpfs",
            libc::USBDEVICE_SUPER_MAGIC => "usbdevice",
            _ => "unknown",
        }
    }

    pub fn fstype(path: &Path) -> io::Result<String> {
        let fs_stat = super::unix::get_statfs(path)?;
        Ok(get_type(fs_stat.f_type, path).into())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CStr;
    use std::io;
    use std::path::Path;

    pub fn fstype(path: &Path) -> io::Result<String> {
        let fs_stat = super::unix::get_statfs(path)?;
        let fs = unsafe { CStr::from_ptr(fs_stat.f_fstypename.as_ptr()) };
        return Ok(fs.to_string_lossy().into());
    }
}

/// Get filesystem type on the given `path`.
///
/// Return "unknown" on unsupported platform.
pub fn fstype(path: impl AsRef<Path>) -> io::Result<String> {
    let path = path.as_ref();

    #[cfg(target_os = "linux")]
    {
        return self::linux::fstype(path);
    }

    #[cfg(target_os = "macos")]
    {
        return self::macos::fstype(path);
    }

    #[cfg(windows)]
    {
        return self::windows::fstype(path);
    }

    #[allow(unreachable_code)]
    {
        return Ok("unknown".to_string());
    }
}
