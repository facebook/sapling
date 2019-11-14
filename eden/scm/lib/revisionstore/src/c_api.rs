/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This module exports some concrete implementations of the revisionstore
//! API for use from C++ code.  The exports in this file match up to the C++
//! header file RevisionStore.h in the top level of this crate.

use std::{
    collections::HashMap,
    ffi::{CStr, OsStr},
    fs,
    os::raw::c_char,
    path::{Path, PathBuf},
    ptr, slice,
    sync::Arc,
};

use failure::Fallible;

use types::{HgId, Key, RepoPath};

use crate::datapack::DataPack;
use crate::datastore::DataStore;
use crate::uniondatastore::UnionDataStore;

pub struct DataPackUnion {
    paths: Vec<PathBuf>,
    packs: HashMap<PathBuf, Arc<DataPack>>,
    store: UnionDataStore<Arc<DataPack>>,
}

/// Returns true if the supplied path has a .datapack extension
fn is_datapack(path: &Path) -> bool {
    if let Some(extension) = path.extension() {
        extension == "datapack"
    } else {
        false
    }
}

enum ScanResult {
    NoChanges,
    ChangesDetected,
}

impl DataPackUnion {
    fn new(paths: Vec<PathBuf>) -> Self {
        Self {
            paths,
            packs: HashMap::new(),
            store: UnionDataStore::new(),
        }
    }

    fn rescan_paths(&mut self) -> ScanResult {
        // first step is to release any unlinked pack files
        let num_before = self.packs.len();
        self.packs.retain(|path, _| path.exists());
        let mut num_changed = num_before - self.packs.len();

        // next step is to discover any new files
        for path in &self.paths {
            match Self::scan_dir(&mut self.packs, path) {
                Err(e) => {
                    eprintln!(
                        "Error while scanning {} for datapack files: {}",
                        path.display(),
                        e
                    );
                }
                Ok(num) => num_changed += num,
            }
        }

        if num_changed == 0 {
            return ScanResult::NoChanges;
        }

        // Re-create the union portion; while we can add elements, there isn't
        // a way to remove them, so we build a new one and populate it.
        // The UnionDataStore is just a Vec of Arc's to our packs, so this is
        // relatively cheap.
        self.store = UnionDataStore::new();

        for pack in self.packs.values() {
            self.store.add(pack.clone());
        }

        ScanResult::ChangesDetected
    }

    fn scan_dir(packs: &mut HashMap<PathBuf, Arc<DataPack>>, path: &Path) -> Fallible<usize> {
        let mut num_changed = 0;
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if is_datapack(&path) {
                if !packs.contains_key(&path) {
                    let pack = Arc::new(DataPack::new(&path)?);
                    packs.insert(path, pack);
                    num_changed += 1;
                }
            }
        }
        Ok(num_changed)
    }

    /// Lookup Key. If the key is missing, scan for changes in the pack files and try once more.
    fn get(&mut self, key: &Key) -> Fallible<Option<Vec<u8>>> {
        match self.store.get(key)? {
            Some(data) => return Ok(Some(data)),
            None => match self.rescan_paths() {
                ScanResult::ChangesDetected => self.store.get(key),
                ScanResult::NoChanges => Ok(None),
            },
        }
    }
}

/// Construct a new datapack store for unix systems.
/// Will panic the program if the paths array or elements
/// of the paths array are null.
/// Returns an instance of DataPackUnion which must be
/// freed using revisionstore_datapackunion_free when it is
/// no longer required.
#[cfg(unix)]
#[no_mangle]
pub extern "C" fn revisionstore_datapackunion_new(
    paths: *const *const c_char,
    num_paths: usize,
) -> *mut DataPackUnion {
    use std::os::unix::ffi::OsStrExt;
    debug_assert!(!paths.is_null());
    let paths = unsafe { slice::from_raw_parts(paths, num_paths) };
    let paths = paths
        .iter()
        .map(|&path_ptr| {
            debug_assert!(
                !path_ptr.is_null(),
                "paths passed to revisionstore_unionstore_new must not be null"
            );
            let path_cstr = unsafe { CStr::from_ptr(path_ptr) };
            let path_bytes = path_cstr.to_bytes();
            Path::new(OsStr::from_bytes(&path_bytes)).to_path_buf()
        })
        .collect();
    let mut store = DataPackUnion::new(paths);
    store.rescan_paths();
    Box::into_raw(Box::new(store))
}

/// Free a DataPackUnion instance created via revisionstore_unionstore_new().
/// Releases all associated resources.
#[no_mangle]
pub extern "C" fn revisionstore_datapackunion_free(store: *mut DataPackUnion) {
    debug_assert!(!store.is_null());
    let store = unsafe { Box::from_raw(store) };
    drop(store);
}

/// Construct an instance of Key from the provided ffi parameters
fn make_key(name: *const u8, name_len: usize, hgid: *const u8, hgid_len: usize) -> Fallible<Key> {
    debug_assert!(!name.is_null());
    debug_assert!(!hgid.is_null());

    let path_slice = unsafe { slice::from_raw_parts(name, name_len) };
    let path = RepoPath::from_utf8(path_slice)?.to_owned();

    let hgid_slice = unsafe { slice::from_raw_parts(hgid, hgid_len) };
    let hgid = HgId::from_slice(hgid_slice)?;

    Ok(Key::new(path, hgid))
}

/// Helper function that performs the get operation, wrapped in a Result
fn datapackunion_get_impl(
    store: *mut DataPackUnion,
    name: *const u8,
    name_len: usize,
    hgid: *const u8,
    hgid_len: usize,
) -> Fallible<Option<Vec<u8>>> {
    debug_assert!(!store.is_null());
    let store = unsafe { &mut *store };
    let key = make_key(name, name_len, hgid, hgid_len)?;
    store.get(&key)
}

#[repr(C)]
pub struct GetData {
    value: *mut Vec<u8>,
    error: *mut String,
    is_key_error: bool,
}

/// Lookup the value corresponding to name/hgid.
/// If the key is present, de-delta and populate `GetData::value`.
/// If the requested key could not be found sets `GetData::is_key_error` to true.
/// If some other error occurred, populates `GetData::error`.
/// The caller is responsible for calling revisionstore_string_free() on
/// the `error` value if it is non-null when this function returns.
/// The caller is responsible for calling revisionstore_bytevec_free() on
/// the `value` if it is non-null when this function returns.
#[no_mangle]
pub extern "C" fn revisionstore_datapackunion_get(
    store: *mut DataPackUnion,
    name: *const u8,
    name_len: usize,
    hgid: *const u8,
    hgid_len: usize,
) -> GetData {
    match datapackunion_get_impl(store, name, name_len, hgid, hgid_len) {
        Ok(Some(data)) => GetData {
            value: Box::into_raw(Box::new(data)),
            error: ptr::null_mut(),
            is_key_error: false,
        },
        Ok(None) => GetData {
            value: ptr::null_mut(),
            error: ptr::null_mut(),
            is_key_error: true,
        },
        Err(err) => GetData {
            value: ptr::null_mut(),
            error: Box::into_raw(Box::new(format!("{}", err))),
            is_key_error: false,
        },
    }
}

/// Dispose of a String object, such as that returned in the `error` parameter
/// of revisionstore_datapackunion_get().
#[no_mangle]
pub extern "C" fn revisionstore_string_free(string: *mut String) {
    debug_assert!(!string.is_null());
    let string = unsafe { Box::from_raw(string) };
    drop(string);
}

/// Returns the pointer to the start of the string data
#[no_mangle]
pub extern "C" fn revisionstore_string_data(string: *mut String) -> ByteData {
    debug_assert!(!string.is_null());
    let string = unsafe { &*string };
    ByteData {
        ptr: string.as_bytes().as_ptr(),
        len: string.len(),
    }
}

/// Dispose of a Vec<u8> object, such as that returned in the `value` parameter
/// of revisionstore_datapackunion_get().
#[no_mangle]
pub extern "C" fn revisionstore_bytevec_free(vec: *mut Vec<u8>) {
    debug_assert!(!vec.is_null());
    let vec = unsafe { Box::from_raw(vec) };
    drop(vec);
}

#[repr(C)]
pub struct ByteData {
    ptr: *const u8,
    len: usize,
}

/// Returns the pointer to the start of the byte data and its size in bytes
#[no_mangle]
pub extern "C" fn revisionstore_bytevec_data(bytes: *mut Vec<u8>) -> ByteData {
    debug_assert!(!bytes.is_null());
    let bytes = unsafe { &*bytes };
    ByteData {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}
