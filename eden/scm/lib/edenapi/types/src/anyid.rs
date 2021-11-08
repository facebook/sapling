/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Gen;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use type_macros::auto_wire;
use types::HgId;

use crate::AnyFileContentId;
use crate::IndexableId;
use crate::UploadToken;

blake2_hash!(BonsaiChangesetId);

#[auto_wire]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
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
pub struct LookupRequest {
    #[id(1)]
    pub id: AnyId,
    #[id(2)]
    pub bubble_id: Option<NonZeroU64>,
}

#[auto_wire]
#[derive(Clone, Serialize, Deserialize, Debug, Eq, PartialEq)]
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
pub struct LookupResponse {
    #[id(1)]
    pub index: usize,
    #[id(2)]
    pub old_token: Option<UploadToken>,
    #[id(3)]
    pub result: LookupResult,
}

impl LookupResponse {
    // TODO(yancouto): This considers old servers, cleanup once it's rolled out for a while.
    pub fn into_result_consider_old(self, ids: &[IndexableId]) -> LookupResult {
        if self.result == LookupResult::default() {
            match self.old_token {
                None => LookupResult::NotPresent(ids[self.index].clone()),
                Some(token) => LookupResult::Present(token),
            }
        } else {
            self.result
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for AnyId {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        use AnyId::*;

        let variant = g.choose(&[0, 1, 2, 3, 4]).unwrap();
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
impl Arbitrary for LookupResult {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        if Arbitrary::arbitrary(g) {
            Self::Present(Arbitrary::arbitrary(g))
        } else {
            Self::NotPresent(Arbitrary::arbitrary(g))
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for LookupResponse {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            index: 0,
            old_token: None,
            result: Arbitrary::arbitrary(g),
        }
    }
}
