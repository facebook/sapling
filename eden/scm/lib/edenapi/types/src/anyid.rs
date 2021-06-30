/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{AnyFileContentId, UploadToken};
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
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

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for AnyId {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        use rand::Rng;
        use AnyId::*;

        let variant = g.gen_range(0, 4);
        match variant {
            0 => AnyFileContentId(Arbitrary::arbitrary(g)),
            1 => HgFilenodeId(Arbitrary::arbitrary(g)),
            2 => HgTreeId(Arbitrary::arbitrary(g)),
            3 => HgChangesetId(Arbitrary::arbitrary(g)),
            _ => unreachable!(),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for LookupRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            id: Arbitrary::arbitrary(g),
        }
    }
}
