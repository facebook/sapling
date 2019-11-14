/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use failure::{format_err, Error};
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
