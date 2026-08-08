/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use winapi::shared::ntdef::HANDLE;
use winapi::shared::ntdef::WCHAR;
use winapi::um::fileapi::FindClose;
use winapi::um::fileapi::FindFirstFileW;
use winapi::um::handleapi::INVALID_HANDLE_VALUE;
use winapi::um::minwinbase::WIN32_FIND_DATAW;
use winapi::um::winbase::FILE_FLAG_BACKUP_SEMANTICS;
use winapi::um::winbase::FILE_FLAG_OPEN_REPARSE_POINT;
use winapi::um::winbase::SetFileShortNameW;
use winapi::um::winnt::DELETE;
use winapi::um::winnt::FILE_SHARE_DELETE;
use winapi::um::winnt::FILE_SHARE_READ;
use winapi::um::winnt::FILE_SHARE_WRITE;

pub(super) fn short_name_at(path: &Path) -> io::Result<Option<OsString>> {
    let path: Vec<WCHAR> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: An all-zero WIN32_FIND_DATAW is a valid output buffer for
    // FindFirstFileW.
    let mut data = unsafe { std::mem::zeroed::<WIN32_FIND_DATAW>() };
    // SAFETY: `path` is a NUL-terminated UTF-16 string, and `data` is a valid
    // writable output buffer.
    let handle = unsafe { FindFirstFileW(path.as_ptr(), &mut data) };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `handle` was returned by FindFirstFileW and has not been closed.
    if unsafe { FindClose(handle) } == 0 {
        return Err(io::Error::last_os_error());
    }

    let len = data
        .cAlternateFileName
        .iter()
        .position(|c| *c == 0)
        .unwrap_or(data.cAlternateFileName.len());
    if len == 0 {
        return Ok(None);
    }
    Ok(Some(OsString::from_wide(&data.cAlternateFileName[..len])))
}

pub(super) fn remove_short_name_at(path: &Path) -> io::Result<()> {
    if short_name_at(path)?.is_none() {
        return Ok(());
    }
    let file = open_for_short_name_removal(path)?;
    set_short_name(&file, &[0])
}

fn open_for_short_name_removal(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

fn set_short_name(file: &File, short_name: &[u16]) -> io::Result<()> {
    debug_assert_eq!(short_name.last(), Some(&0));
    // SAFETY: `file` owns a valid handle for the duration of the call, and
    // `short_name` points to a NUL-terminated UTF-16 string.
    if unsafe { SetFileShortNameW(file.as_raw_handle() as HANDLE, short_name.as_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::iter;

    use super::*;

    #[test]
    fn remove_existing_short_name() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("long-file-name-for-8dot3");
        std::fs::write(&path, [])?;
        assert_remove_existing_short_name(&path, "LONG-F~1")
    }

    #[test]
    fn remove_existing_directory_short_name() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("long-directory-name-for-8dot3");
        std::fs::create_dir(&path)?;
        assert_remove_existing_short_name(&path, "LONG-D~1")
    }

    fn assert_remove_existing_short_name(path: &Path, short_name: &str) -> io::Result<()> {
        let file = open_for_short_name_removal(path)?;
        let short_path = path.parent().unwrap().join(short_name);
        let short_name_wide: Vec<u16> = short_name.encode_utf16().chain(iter::once(0)).collect();
        set_short_name(&file, &short_name_wide)?;
        assert_eq!(
            short_name_at(path)?.as_deref(),
            Some(OsStr::new(short_name))
        );
        std::fs::metadata(&short_path)?;

        remove_short_name_at(path)?;
        assert_eq!(short_name_at(path)?, None);
        assert_eq!(
            std::fs::metadata(short_path).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        std::fs::metadata(path)?;
        Ok(())
    }
}
