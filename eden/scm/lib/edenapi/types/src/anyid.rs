/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{AnyFileContentId, UploadToken};
use serde_derive::{Deserialize, Serialize};
use types::HgId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnyId {
    AnyFileContentId(AnyFileContentId),
    HgFilenodeId(HgId),
    HgTreeId(HgId),
    HgChangesetId(HgId),
}

impl Default for AnyId {
    fn default() -> Self {
        Self::AnyFileContentId(AnyFileContentId::default())
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct LookupRequest {
    pub id: AnyId,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct LookupResponse {
    pub index: usize,
    pub token: Option<UploadToken>,
}
