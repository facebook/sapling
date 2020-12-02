/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! These types are only exposed to C / C++ behind a pointer. They are used for
//! references into data types owned by Rust. C functions for opaque types that
//! are not defined in this crate are also included here.

use std::ptr;

use libc::size_t;

use crate::{ContentId, FileType, HgId, Parents, Sha1, Sha256};
use anyhow::Result;

// Opaque types from other crates
pub use edenapi::EdenApiError;
pub use edenapi_types::{EdenApiServerError, FileMetadata, TreeChildEntry, TreeEntry};
pub use types::Key as ApiKey;

/// Methods for ApiKey
#[no_mangle]
pub extern "C" fn rust_key_get_hgid(k: *const ApiKey) -> HgId {
    assert!(!k.is_null());
    let k = unsafe { &*k };
    k.hgid.into()
}
#[no_mangle]
pub extern "C" fn rust_key_get_path_len(k: *const ApiKey) -> size_t {
    assert!(!k.is_null());
    let k = unsafe { &*k };
    let s: &str = k.path.as_ref();
    s.len()
}
#[no_mangle]
pub extern "C" fn rust_key_get_path(k: *const ApiKey) -> *const u8 {
    assert!(!k.is_null());
    let k = unsafe { &*k };
    let s: &str = k.path.as_ref();
    s.as_ptr()
}

#[no_mangle]
pub extern "C" fn rust_treeentry_has_key(entry: *const TreeEntry) -> bool {
    assert!(!entry.is_null());
    true
}
#[no_mangle]
pub extern "C" fn rust_treeentry_get_key(entry: *const TreeEntry) -> *const ApiKey {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    &entry.key
}

#[no_mangle]
pub extern "C" fn rust_treeentry_has_data(entry: *const TreeEntry) -> bool {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    entry.data.is_some()
}
#[no_mangle]
pub extern "C" fn rust_treeentry_get_data(entry: *const TreeEntry) -> *const u8 {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    entry.data.as_ref().unwrap().as_ptr()
}
#[no_mangle]
pub extern "C" fn rust_treeentry_get_len(entry: *const TreeEntry) -> size_t {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    entry.data.as_ref().unwrap().len()
}

#[no_mangle]
pub extern "C" fn rust_treeentry_has_parents(entry: *const TreeEntry) -> bool {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    entry.parents.is_some()
}
#[no_mangle]
pub extern "C" fn rust_treeentry_get_parents(entry: *const TreeEntry) -> Parents {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    entry.parents.unwrap().into()
}

#[no_mangle]
pub extern "C" fn rust_treeentry_has_children(entry: *const TreeEntry) -> bool {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    entry.children.is_some()
}
#[no_mangle]
pub extern "C" fn rust_treeentry_get_children_len(entry: *const TreeEntry) -> bool {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    entry.children.is_some()
}

#[no_mangle]
pub extern "C" fn rust_treeentry_get_children(
    entry: *const TreeEntry,
) -> *const Vec<Result<TreeChildEntry, EdenApiServerError>> {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    entry.children.as_ref().unwrap()
}

#[no_mangle]
pub extern "C" fn rust_treechildentry_is_file(entry: *const TreeChildEntry) -> bool {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    if let TreeChildEntry::File(_) = entry {
        true
    } else {
        false
    }
}

#[no_mangle]
pub extern "C" fn rust_treechildentry_get_file_key(entry: *const TreeChildEntry) -> *const ApiKey {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    if let TreeChildEntry::File(f) = entry {
        &f.key
    } else {
        ptr::null()
    }
}
#[no_mangle]
pub extern "C" fn rust_treechildentry_has_file_metadata(entry: *const TreeChildEntry) -> bool {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    if let TreeChildEntry::File(f) = entry {
        if f.file_metadata.is_some() {
            true
        } else {
            false
        }
    } else {
        false
    }
}
#[no_mangle]
pub extern "C" fn rust_treechildentry_get_file_metadata(
    entry: *const TreeChildEntry,
) -> *const FileMetadata {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    if let TreeChildEntry::File(f) = entry {
        if let Some(meta) = f.file_metadata {
            &meta
        } else {
            ptr::null()
        }
    } else {
        ptr::null()
    }
}

#[no_mangle]
pub extern "C" fn rust_treechildentry_is_directory(entry: *const TreeChildEntry) -> bool {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    if let TreeChildEntry::Directory(_) = entry {
        true
    } else {
        false
    }
}

#[no_mangle]
pub extern "C" fn rust_treechildentry_get_directory_key(
    entry: *const TreeChildEntry,
) -> *const ApiKey {
    assert!(!entry.is_null());
    let entry = unsafe { &*entry };
    if let TreeChildEntry::Directory(d) = entry {
        &d.key
    } else {
        ptr::null()
    }
}

#[no_mangle]
pub extern "C" fn rust_filemetadata_has_revisionstore_flags(m: *const FileMetadata) -> bool {
    assert!(!m.is_null());
    let m = unsafe { &*m };
    m.revisionstore_flags.is_some()
}

#[no_mangle]
pub extern "C" fn rust_filemetadata_has_content_id(m: *const FileMetadata) -> bool {
    assert!(!m.is_null());
    let m = unsafe { &*m };
    m.content_id.is_some()
}

#[no_mangle]
pub extern "C" fn rust_filemetadata_has_file_type(m: *const FileMetadata) -> bool {
    assert!(!m.is_null());
    let m = unsafe { &*m };
    m.file_type.is_some()
}

#[no_mangle]
pub extern "C" fn rust_filemetadata_has_size(m: *const FileMetadata) -> bool {
    assert!(!m.is_null());
    let m = unsafe { &*m };
    m.size.is_some()
}

#[no_mangle]
pub extern "C" fn rust_filemetadata_has_content_sha1(m: *const FileMetadata) -> bool {
    assert!(!m.is_null());
    let m = unsafe { &*m };
    m.content_sha1.is_some()
}

#[no_mangle]
pub extern "C" fn rust_filemetadata_has_content_sha256(m: *const FileMetadata) -> bool {
    assert!(!m.is_null());
    let m = unsafe { &*m };
    m.content_sha256.is_some()
}

#[no_mangle]
pub extern "C" fn rust_filemetadata_get_revisionstore_flags(m: *const FileMetadata) -> u64 {
    assert!(!m.is_null());
    let m = unsafe { &*m };
    m.revisionstore_flags.unwrap()
}

#[no_mangle]
pub extern "C" fn rust_filemetadata_get_content_id(m: *const FileMetadata) -> ContentId {
    assert!(!m.is_null());
    let m = unsafe { &*m };
    m.content_id.unwrap().into()
}

#[no_mangle]
pub extern "C" fn rust_filemetadata_get_file_type(m: *const FileMetadata) -> FileType {
    assert!(!m.is_null());
    let m = unsafe { &*m };
    m.file_type.unwrap().into()
}

#[no_mangle]
pub extern "C" fn rust_filemetadata_get_size(m: *const FileMetadata) -> u64 {
    assert!(!m.is_null());
    let m = unsafe { &*m };
    m.size.unwrap()
}

#[no_mangle]
pub extern "C" fn rust_filemetadata_get_content_sha1(m: *const FileMetadata) -> Sha1 {
    assert!(!m.is_null());
    let m = unsafe { &*m };
    m.content_sha1.unwrap().into()
}

#[no_mangle]
pub extern "C" fn rust_filemetadata_get_content_sha256(m: *const FileMetadata) -> Sha256 {
    assert!(!m.is_null());
    let m = unsafe { &*m };
    m.content_sha256.unwrap().into()
}
