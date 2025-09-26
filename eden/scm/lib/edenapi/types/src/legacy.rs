/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use minibytes::Bytes;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use type_macros::auto_wire;

use crate::ServerError;

#[auto_wire]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct StreamingChangelogRequest {
    #[id(0)]
    pub tag: Option<String>,
}

#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum StreamingChangelogData {
    #[id(1)]
    Metadata(Metadata),
    #[id(2)]
    IndexBlobChunk(StreamingChangelogBlob),
    #[id(3)]
    DataBlobChunk(StreamingChangelogBlob),
}

// autowire requires a default value, but this should be unused
impl Default for StreamingChangelogData {
    fn default() -> Self {
        Self::Metadata(Metadata {
            index_size: 0,
            data_size: 0,
        })
    }
}

#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct StreamingChangelogBlob {
    #[id(0)]
    pub chunk: Bytes,
    #[id(1)]
    pub chunk_id: u64,
}

#[auto_wire]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct Metadata {
    #[id(0)]
    pub index_size: u64,
    #[id(1)]
    pub data_size: u64,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for StreamingChangelogBlob {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let bytes: Vec<u8> = Arbitrary::arbitrary(g);
        Self {
            chunk: bytes.into(),
            chunk_id: Arbitrary::arbitrary(g),
        }
    }
}

#[auto_wire]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct StreamingChangelogResponse {
    #[id(0)]
    #[no_default]
    pub data: Result<StreamingChangelogData, ServerError>,
}
