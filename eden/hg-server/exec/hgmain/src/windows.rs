/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use std::path::Path;
use winapi::shared::minwindef::DWORD;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::{SetHandleInformation, INVALID_HANDLE_VALUE};
use winapi::um::processenv::GetStdHandle;
use winapi::um::winbase::{
    HANDLE_FLAG_INHERIT, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use winapi::um::winnt::HANDLE;

fn std_handle(handle: DWORD) -> Result<HANDLE, Error> {
    let res = unsafe { GetStdHandle(handle) };
    if res == INVALID_HANDLE_VALUE {
        return Err(format_err!("failed to call GetStdHandle: {:?}", unsafe {
            GetLastError()
        }));
    }
    Ok(res)
}

fn set_handle_inheritability(handle: HANDLE, inherit: bool) -> Result<(), Error> {
    if handle.is_null() {
        return Ok(());
    }
    let flags = if inherit { HANDLE_FLAG_INHERIT } else { 0 };
    if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, flags) } == 0 {
        return Err(format_err!(
            "failed to call SetHandleInformation: {:?}",
            unsafe { GetLastError() }
        ));
    }
    Ok(())
}

pub fn disable_standard_handle_inheritability() -> Result<(), Error> {
    set_handle_inheritability(std_handle(STD_INPUT_HANDLE)?, false)?;
    set_handle_inheritability(std_handle(STD_OUTPUT_HANDLE)?, false)?;
    set_handle_inheritability(std_handle(STD_ERROR_HANDLE)?, false)?;
    Ok(())
}

/// Test if the given path is backed by EdenFS on Windows and if EdenFS is currently stopped. This
/// function will return false if the repository is not backed by EdenFS.
pub fn is_edenfs_stopped(path: &Path) -> bool {
    let check_dir = path.join(".EDEN_TEST_NON_EXISTENCE_PATH");

    if let Err(err) = std::fs::read_dir(&check_dir) {
        if let Some(code) = err.raw_os_error() {
            // `ERROR_FILE_SYSTEM_VIRTUALIZATION_UNAVAILABLE`: unfortunately this is an
            // undocumented error code. When EdenFS is not running, `readdir` will fail with this
            // error code since ProjectedFS has nowhere to look for the directory information.
            return code == 369;
        }
    }

    false
}
