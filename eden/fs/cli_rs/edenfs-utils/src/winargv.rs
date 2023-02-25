/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg(windows)]

use std::ffi::OsStr;
use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::ffi::OsStringExt;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::shellapi::CommandLineToArgvW;
use winapi::um::winbase::LocalFree;

/// Parses a Windows command line into an argv vector of program name followed
/// by zero or more command-line arguments.
pub fn command_line_to_argv(command_line: &OsStr) -> Result<Vec<OsString>> {
    if command_line.is_empty() {
        // CommandLineToArgvW assumes the current executable file if passed an
        // empty string, but we don't want that behavior.
        return Ok(vec![]);
    }

    let command_line_w = command_line
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();

    let mut argv = Vec::<OsString>::new();

    let mut num_args: i32 = 0;
    let argv_w = LocalPtr(unsafe { CommandLineToArgvW(command_line_w.as_ptr(), &mut num_args) });
    if argv_w.0.is_null() {
        return Err(anyhow!("CommandLineToArgvW failed: {:?}", unsafe {
            GetLastError()
        }));
    }

    for i in 0..num_args {
        let arg_offset: isize = i.try_into()?;
        let arg_w = unsafe { *argv_w.0.offset(arg_offset) } as *const u16;
        let arg_w_slice = unsafe { null_terminated_slice(arg_w)? };
        argv.push(OsString::from_wide(arg_w_slice));
    }
    Ok(argv)
}

/// Owned pointer that needs to be freed with LocalFree.
struct LocalPtr<T>(*mut T);

impl<T> Drop for LocalPtr<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0 as *mut _) };
        }
    }
}

/// Given a pointer to a null-terminated wide character string, returns a slice
/// of the string, excluding the null wide character.  Behavior is undefined if
/// given a pointer to a non-null-terminated string.
unsafe fn null_terminated_slice<'a>(ptr: *const u16) -> Result<&'a [u16]> {
    let mut i = 0isize;
    loop {
        if *ptr.offset(i) == 0u16 {
            break;
        }
        i += 1;
    }
    if *ptr.offset(i) != 0u16 {
        bail!("No null terminator found");
    }

    let slice_size: usize = i.try_into()?;
    Ok(std::slice::from_raw_parts(ptr, slice_size))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::ffi::OsString;
    use std::str::FromStr;

    use anyhow::Result;

    use super::command_line_to_argv;
    use super::null_terminated_slice;

    #[test]
    fn test_command_line_to_argv() -> Result<()> {
        assert!(command_line_to_argv(OsStr::new(""))?.is_empty());
        assert_eq!(
            command_line_to_argv(OsStr::new("C:\\Windows\\system32\\svchost.exe"))?,
            vec![OsString::from_str("C:\\Windows\\system32\\svchost.exe")?]
        );
        assert_eq!(
            command_line_to_argv(OsStr::new("foo.exe bar.txt"))?,
            vec![
                OsString::from_str("foo.exe")?,
                OsString::from_str("bar.txt")?
            ]
        );

        Ok(())
    }

    #[test]
    fn test_null_terminated_slice() -> Result<()> {
        unsafe {
            assert!(null_terminated_slice(vec![0u16].as_ptr())?.is_empty());
            assert_eq!(
                null_terminated_slice(vec![1u16, 0u16].as_ptr())?,
                vec![1u16]
            );
            assert_eq!(
                null_terminated_slice(vec![1u16, 2u16, 0u16].as_ptr())?,
                vec![1u16, 2u16]
            );
        }

        Ok(())
    }
}
