/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This module exports some symbols to allow calling the config parser from C/C++
use std::ffi::{CStr, OsStr};
use std::os::raw::c_char;
use std::path::Path;
use std::ptr;
use std::slice;

use bytes::Bytes;

use crate::config::{ConfigSet, Options};
use crate::error::Error;
use crate::hg::ConfigSetHgExt;
use crate::hg::OptionsHgExt;

/// Create and return a new, empty ConfigSet
#[no_mangle]
pub extern "C" fn hgrc_configset_new() -> *mut ConfigSet {
    Box::into_raw(Box::new(ConfigSet::new()))
}

/// Free a ConfigSet instance created via hgrc_configset_new().
/// Releases all associated resources.
#[no_mangle]
pub extern "C" fn hgrc_configset_free(cfg: *mut ConfigSet) {
    debug_assert!(!cfg.is_null());
    let cfg = unsafe { Box::from_raw(cfg) };
    drop(cfg);
}

fn errors_to_bytes(errors: Vec<Error>) -> *mut Bytes {
    if errors.is_empty() {
        // Success!
        return ptr::null_mut();
    }

    // Failed; convert the errors into an error string
    let mut error_text = String::new();
    for (idx, err) in errors.iter().enumerate() {
        if idx > 0 {
            error_text.push_str("\n");
        }
        error_text.push_str(&err.to_string());
    }

    Box::into_raw(Box::new(error_text.into()))
}

fn load_path(cfg: &mut ConfigSet, path: &Path) -> *mut Bytes {
    let errors = cfg.load_path(path, &Options::new().process_hgplain());

    errors_to_bytes(errors)
}

/// Attempt to load and parse the config file at the specified path.
/// If successful, returns a nullptr.
/// Returns a Bytes object containing the error reason on failure; the
/// error object is UTF-8 encoded text, and errors can span multiple lines.
#[cfg(unix)]
#[no_mangle]
pub extern "C" fn hgrc_configset_load_path(cfg: *mut ConfigSet, path: *const c_char) -> *mut Bytes {
    debug_assert!(!path.is_null());
    debug_assert!(!cfg.is_null());

    use std::os::unix::ffi::OsStrExt;

    let path_cstr = unsafe { CStr::from_ptr(path) };
    let path_bytes = path_cstr.to_bytes();
    let path = Path::new(OsStr::from_bytes(&path_bytes));

    let cfg = unsafe { &mut *cfg };

    load_path(cfg, path)
}

/// Load system config files
#[no_mangle]
pub extern "C" fn hgrc_configset_load_system(cfg: *mut ConfigSet) -> *mut Bytes {
    debug_assert!(!cfg.is_null());
    let cfg = unsafe { &mut *cfg };

    // Forces datapath to be the empty string as it doesn't
    // appear to play a useful role in simply resolving config
    // settings for Eden.
    errors_to_bytes(cfg.load_system())
}

/// Load user config files
#[no_mangle]
pub extern "C" fn hgrc_configset_load_user(cfg: *mut ConfigSet) -> *mut Bytes {
    debug_assert!(!cfg.is_null());
    let cfg = unsafe { &mut *cfg };

    errors_to_bytes(cfg.load_user())
}

/// Returns a Bytes object holding the configuration value for the corresponding
/// section name and key.   If there is no matching section/key pair, returns nullptr.
#[no_mangle]
pub extern "C" fn hgrc_configset_get(
    cfg: *const ConfigSet,
    section: *const u8,
    section_len: usize,
    name: *const u8,
    name_len: usize,
) -> *mut Bytes {
    debug_assert!(!section.is_null());
    debug_assert!(!name.is_null());
    debug_assert!(!cfg.is_null());

    let section = unsafe { slice::from_raw_parts(section, section_len) };
    let name = unsafe { slice::from_raw_parts(name, name_len) };
    let cfg = unsafe { &*cfg };

    match cfg.get(section, name) {
        None => ptr::null_mut(),
        Some(bytes) => Box::into_raw(Box::new(bytes)),
    }
}

#[repr(C)]
pub struct ByteData {
    ptr: *const u8,
    len: usize,
}

/// Returns the data pointer and length for a Bytes object, suitable for constructing
/// a folly::ByteRange.
#[no_mangle]
pub extern "C" fn hgrc_bytes_data(bytes: *const Bytes) -> ByteData {
    debug_assert!(!bytes.is_null());
    let bytes = unsafe { &*bytes };
    ByteData {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

/// Frees a Bytes object, releasing any associated resources
#[no_mangle]
pub extern "C" fn hgrc_bytes_free(bytes: *mut Bytes) {
    debug_assert!(!bytes.is_null());
    let bytes = unsafe { Box::from_raw(bytes) };
    drop(bytes);
}
