/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ptr;

use anyhow::Error;

use edenapi::Client;

use crate::{EdenApiServerError, OwnedString, TreeChildEntry, TreeEntry};

trait ResultExt {
    fn unwrap_err_display(&self) -> String;
    fn unwrap_err_debug(&self) -> String;
}

impl<T, E: std::fmt::Display + std::fmt::Debug> ResultExt for Result<T, E> {
    fn unwrap_err_display(&self) -> String {
        format!("{}", self.as_ref().err().unwrap())
    }

    fn unwrap_err_debug(&self) -> String {
        format!("{:#?}", self.as_ref().err().unwrap())
    }
}

// Monomorphization for Result<TreeEntry, EdenApiServerError>
#[no_mangle]
pub extern "C" fn rust_result_treeentry_ok(
    r: *const Result<TreeEntry, EdenApiServerError>,
) -> *const TreeEntry {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    if let Ok(ref v) = r { v } else { ptr::null() }
}

#[no_mangle]
pub extern "C" fn rust_result_treeentry_is_err(
    r: *const Result<TreeEntry, EdenApiServerError>,
) -> bool {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    if r.is_err() { true } else { false }
}

#[no_mangle]
pub extern "C" fn rust_result_treeentry_err_display(
    r: *const Result<TreeEntry, EdenApiServerError>,
) -> OwnedString {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    r.unwrap_err_display().into()
}

#[no_mangle]
pub extern "C" fn rust_result_treeentry_err_debug(
    r: *const Result<TreeEntry, EdenApiServerError>,
) -> OwnedString {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    r.unwrap_err_debug().into()
}

// Monomorphization for Result<Vec<Result<TreeEntry, EdenApiServerError>>, Error>
#[no_mangle]
pub extern "C" fn rust_result_entries_ok(
    r: *const Result<Vec<Result<TreeEntry, EdenApiServerError>>, Error>,
) -> *const Vec<Result<TreeEntry, EdenApiServerError>> {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    if let Ok(ref v) = r { v } else { ptr::null() }
}

#[no_mangle]
pub extern "C" fn rust_result_entries_is_err(
    r: *const Result<Vec<Result<TreeEntry, EdenApiServerError>>, Error>,
) -> bool {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    if r.is_err() { true } else { false }
}

#[no_mangle]
pub extern "C" fn rust_result_entries_err_display(
    r: *const Result<Vec<Result<TreeEntry, EdenApiServerError>>, Error>,
) -> OwnedString {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    r.unwrap_err_display().into()
}

#[no_mangle]
pub extern "C" fn rust_result_entries_err_debug(
    r: *const Result<Vec<Result<TreeEntry, EdenApiServerError>>, Error>,
) -> OwnedString {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    r.unwrap_err_debug().into()
}

// Monomorphization for Result<Client, Error>
#[no_mangle]
pub extern "C" fn rust_result_client_ok(r: *const Result<Client, Error>) -> *const Client {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    if let Ok(ref v) = r { v } else { ptr::null() }
}

#[no_mangle]
pub extern "C" fn rust_result_client_is_err(r: *const Result<Client, Error>) -> bool {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    if r.is_err() { true } else { false }
}

#[no_mangle]
pub extern "C" fn rust_result_client_err_display(r: *const Result<Client, Error>) -> OwnedString {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    r.unwrap_err_display().into()
}

#[no_mangle]
pub extern "C" fn rust_result_client_err_debug(r: *const Result<Client, Error>) -> OwnedString {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    r.unwrap_err_debug().into()
}

// Monomorphization for Result<TreeChildEntry, EdenApiServerError>
#[no_mangle]
pub extern "C" fn rust_result_treechildentry_ok(
    r: *const Result<TreeChildEntry, EdenApiServerError>,
) -> *const TreeChildEntry {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    if let Ok(ref v) = r { v } else { ptr::null() }
}

#[no_mangle]
pub extern "C" fn rust_result_treechildentry_is_err(
    r: *const Result<TreeChildEntry, EdenApiServerError>,
) -> bool {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    if r.is_err() { true } else { false }
}

#[no_mangle]
pub extern "C" fn rust_result_treechildentry_err_display(
    r: *const Result<TreeChildEntry, EdenApiServerError>,
) -> OwnedString {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    r.unwrap_err_display().into()
}

#[no_mangle]
pub extern "C" fn rust_result_treechildentry_err_debug(
    r: *const Result<TreeChildEntry, EdenApiServerError>,
) -> OwnedString {
    assert!(!r.is_null());
    let r = unsafe { &*r };
    r.unwrap_err_debug().into()
}
