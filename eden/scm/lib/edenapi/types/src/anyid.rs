/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

use crate::{AnyFileContentId, UploadToken};
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::{Arbitrary, Gen};
#[cfg(any(test, feature = "for-tests"))]
use serde_derive::{Deserialize, Serialize};
use type_macros::auto_wire;
use types::HgId;

blake2_hash!(BonsaiChangesetId);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum AnyId {
    AnyFileContentId(AnyFileContentId),
    HgFilenodeId(HgId),
    HgTreeId(HgId),
    HgChangesetId(HgId),
    BonsaiChangesetId(BonsaiChangesetId),
}

impl Default for AnyId {
    fn default() -> Self {
        Self::AnyFileContentId(AnyFileContentId::default())
    }
}

#[auto_wire]
#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct LookupRequest {
    #[id(1)]
    pub id: AnyId,
    #[id(2)]
    pub bubble_id: Option<NonZeroU64>,
}

#[auto_wire]
#[derive(Clone, Serialize, Deserialize, Default, Debug, Eq, PartialEq)]
pub struct LookupResponse {
    #[id(1)]
    pub index: usize,
    #[id(2)]
    pub token: Option<UploadToken>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for AnyId {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        use AnyId::*;

        let variant = u32::arbitrary(g) % 5;
        match variant {
            0 => AnyFileContentId(Arbitrary::arbitrary(g)),
            1 => HgFilenodeId(Arbitrary::arbitrary(g)),
            2 => HgTreeId(Arbitrary::arbitrary(g)),
            3 => HgChangesetId(Arbitrary::arbitrary(g)),
            4 => BonsaiChangesetId(Arbitrary::arbitrary(g)),
            _ => unreachable!(),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for LookupRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            id: Arbitrary::arbitrary(g),
            bubble_id: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for LookupResponse {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            index: Arbitrary::arbitrary(g),
            token: Arbitrary::arbitrary(g),
        }
    }
}
