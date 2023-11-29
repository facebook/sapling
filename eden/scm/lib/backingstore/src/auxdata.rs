/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use storemodel::FileAuxData as ScmStoreFileAuxData;

use crate::ffi::ffi::FileAuxData;

impl From<ScmStoreFileAuxData> for FileAuxData {
    fn from(v: ScmStoreFileAuxData) -> Self {
        FileAuxData {
            total_size: v.total_size,
            content_id: v.content_id.into(),
            content_sha1: v.sha1.into(),
            content_sha256: v.sha256.into_byte_array(),
            has_blake3: v.seeded_blake3.is_some(),
            content_blake3: v
                .seeded_blake3
                .map_or([0u8; 32], |content_blake3| content_blake3.into_byte_array()),
        }
    }
}
