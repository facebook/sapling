/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use revisionstore::scmstore::file::FileAuxData as ScmStoreFileAuxData;

use crate::raw::CBytes;

#[repr(C)]
pub struct FileAuxData {
    total_size: u64,
    content_id: CBytes,
    content_sha1: CBytes,
    content_sha256: CBytes,
}

impl From<ScmStoreFileAuxData> for FileAuxData {
    fn from(v: ScmStoreFileAuxData) -> Self {
        // TODO(meyer): Yet more unnecessary allocation, need to convert backing to cxx
        FileAuxData {
            total_size: v.total_size,
            content_id: v.content_id.as_ref().to_vec().into(),
            content_sha1: v.content_sha1.as_ref().to_vec().into(),
            content_sha256: v.content_sha256.as_ref().to_vec().into(),
        }
    }
}

#[no_mangle]
pub extern "C" fn sapling_file_aux_free(aux: *mut FileAuxData) {
    assert!(!aux.is_null());
    let aux = unsafe { Box::from_raw(aux) };
    drop(aux);
}
