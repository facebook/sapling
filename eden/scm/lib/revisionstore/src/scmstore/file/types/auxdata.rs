/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use edenapi_types::ContentId;
use edenapi_types::FileAuxData as EdenApiFileAuxData;
use edenapi_types::Sha1;
use serde::Deserialize;
use serde::Serialize;
use types::Sha256;

use crate::indexedlogauxstore::Entry as AuxDataEntry;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FileAuxData {
    pub total_size: u64,
    pub content_id: ContentId,
    pub content_sha1: Sha1,
    pub content_sha256: Sha256,
}

impl From<AuxDataEntry> for FileAuxData {
    fn from(v: AuxDataEntry) -> Self {
        FileAuxData {
            total_size: v.total_size() as u64,
            content_id: v.content_id(),
            content_sha1: v.content_sha1(),
            content_sha256: Sha256::from_byte_array(v.content_sha256().into()),
        }
    }
}

impl From<FileAuxData> for AuxDataEntry {
    fn from(v: FileAuxData) -> Self {
        AuxDataEntry {
            total_size: v.total_size,
            content_id: v.content_id,
            content_sha1: v.content_sha1,
            content_sha256: v.content_sha256.into_inner().into(),
        }
    }
}

impl From<EdenApiFileAuxData> for FileAuxData {
    fn from(v: EdenApiFileAuxData) -> Self {
        FileAuxData {
            total_size: v.total_size,
            content_id: v.content_id,
            content_sha1: v.sha1,
            content_sha256: Sha256::from_byte_array(v.sha256.into()),
        }
    }
}

impl From<FileAuxData> for EdenApiFileAuxData {
    fn from(v: FileAuxData) -> Self {
        EdenApiFileAuxData {
            total_size: v.total_size,
            content_id: v.content_id,
            sha1: v.content_sha1,
            sha256: v.content_sha256.into_inner().into(),
        }
    }
}
