/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use type_macros::auto_wire;
use types::HgId;

use crate::commitid::BonsaiChangesetId;
use crate::AnyFileContentId;
use crate::IndexableId;
use crate::UploadToken;

#[auto_wire]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum AnyId {
    #[id(1)]
    AnyFileContentId(AnyFileContentId),
    #[id(2)]
    HgFilenodeId(HgId),
    #[id(3)]
    HgTreeId(HgId),
    #[id(4)]
    HgChangesetId(HgId),
    #[id(5)]
    BonsaiChangesetId(BonsaiChangesetId),
}

impl Default for AnyId {
    fn default() -> Self {
        Self::AnyFileContentId(AnyFileContentId::default())
    }
}

#[auto_wire]
#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct LookupRequest {
    #[id(1)]
    pub id: AnyId,
    #[id(2)]
    pub bubble_id: Option<NonZeroU64>,
    /// If present and the original id is not, lookup will also look into this
    /// bubble, and if the id is present, copy it to the requested bubble.
    #[id(3)]
    pub copy_from_bubble_id: Option<NonZeroU64>,
}

#[auto_wire]
#[derive(Clone, Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum LookupResult {
    /// Id was present, upload token for it is returned
    #[id(1)]
    Present(UploadToken),
    /// Id was not present, only its id is returned
    #[id(2)]
    NotPresent(IndexableId),
    // Possible to add an Error variant in the future if we don't want to
    // swallow the errors
}

impl Default for LookupResult {
    fn default() -> Self {
        Self::NotPresent(Default::default())
    }
}

#[auto_wire]
#[derive(Clone, Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct LookupResponse {
    #[id(3)]
    pub result: LookupResult,
}
