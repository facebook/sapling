/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};

use crate::{
    tree::{TreeEntry, TreeRequest},
    wire::{
        is_default, ToApi, ToWire, WireDirectoryMetadata, WireDirectoryMetadataRequest,
        WireEdenApiServerError, WireFileMetadata, WireFileMetadataRequest, WireKey, WireParents,
        WireToApiConversionError,
    },
    EdenApiServerError,
};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WireTreeEntry {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    key: Option<WireKey>,

    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    data: Option<Bytes>,

    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    parents: Option<WireParents>,

    #[serde(rename = "3", default, skip_serializing_if = "is_default")]
    file_metadata: Option<WireFileMetadata>,

    #[serde(rename = "4", default, skip_serializing_if = "is_default")]
    directory_metadata: Option<WireDirectoryMetadata>,

    #[serde(rename = "5", default, skip_serializing_if = "is_default")]
    children: Option<Vec<WireTreeEntry>>,

    #[serde(rename = "6", default, skip_serializing_if = "is_default")]
    error: Option<WireEdenApiServerError>,
}

impl ToWire for Result<TreeEntry, EdenApiServerError> {
    type Wire = WireTreeEntry;

    fn to_wire(self) -> Self::Wire {
        match self {
            Ok(t) => WireTreeEntry {
                key: Some(t.key.to_wire()),
                data: t.data,
                parents: t.parents.to_wire(),
                file_metadata: t.file_metadata.to_wire(),
                directory_metadata: t.directory_metadata.to_wire(),
                children: t.children.to_wire(),
                error: None,
            },
            Err(e) => WireTreeEntry {
                key: e.key.to_wire(),
                error: Some(e.err.to_wire()),
                ..Default::default()
            },
        }
    }
}

impl ToApi for WireTreeEntry {
    type Api = Result<TreeEntry, EdenApiServerError>;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(if let (key, Some(err)) = (self.key.clone(), self.error) {
            Err(EdenApiServerError {
                key: key.to_api()?,
                err: err.to_api()?,
            })
        } else {
            Ok(TreeEntry {
                key: self
                    .key
                    .to_api()?
                    .ok_or(WireToApiConversionError::CannotPopulateRequiredField("key"))?,
                data: self.data,
                parents: self.parents.to_api()?,
                file_metadata: self.file_metadata.to_api()?,
                directory_metadata: self.directory_metadata.to_api()?,
                children: self.children.to_api()?,
            })
        })
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireTreeKeysQuery {
    pub keys: Vec<WireKey>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum WireTreeQuery {
    #[serde(rename = "1")]
    ByKeys(WireTreeKeysQuery),

    #[serde(other, rename = "0")]
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireTreeAttributesRequest {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    with_data: bool,

    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    with_parents: bool,

    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    with_file_metadata: Option<WireFileMetadataRequest>,

    #[serde(rename = "3", default, skip_serializing_if = "is_default")]
    with_directory_metadata: Option<WireDirectoryMetadataRequest>,

    #[serde(rename = "4", default, skip_serializing_if = "is_default")]
    with_children: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireTreeRequest {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    query: Option<WireTreeQuery>,

    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    attributes: Option<WireTreeAttributesRequest>,
}

impl ToWire for TreeRequest {
    type Wire = WireTreeRequest;

    fn to_wire(self) -> Self::Wire {
        WireTreeRequest {
            query: Some(WireTreeQuery::ByKeys(WireTreeKeysQuery {
                keys: self.keys.to_wire(),
            })),

            attributes: Some(WireTreeAttributesRequest {
                with_data: true,
                with_parents: true,
                with_file_metadata: self.with_file_metadata.to_wire(),
                with_directory_metadata: self.with_directory_metadata.to_wire(),
                with_children: true,
            }),
        }
    }
}

impl ToApi for WireTreeRequest {
    type Api = TreeRequest;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(TreeRequest {
            keys: match self.query {
                Some(WireTreeQuery::ByKeys(kq)) => kq.keys.to_api()?,
                Some(_) => {
                    return Err(WireToApiConversionError::UnrecognizedEnumVariant(
                        "WireTreeQuery",
                    ));
                }
                None => {
                    return Err(WireToApiConversionError::CannotPopulateRequiredField(
                        "keys",
                    ));
                }
            },
            with_file_metadata: self
                .attributes
                .as_ref()
                .and_then(|a| a.with_file_metadata)
                .to_api()?,
            with_directory_metadata: self
                .attributes
                .as_ref()
                .and_then(|a| a.with_directory_metadata)
                .to_api()?,
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireTreeEntry {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let bytes: Option<Vec<u8>> = Arbitrary::arbitrary(g);
        Self {
            key: Arbitrary::arbitrary(g),
            data: bytes.map(|b| Bytes::from(b)),
            parents: Arbitrary::arbitrary(g),
            file_metadata: Arbitrary::arbitrary(g),
            directory_metadata: Arbitrary::arbitrary(g),
            // Not recursing here because it causes Quickcheck to overflow the stack
            children: None,
            // TODO
            error: None,
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireTreeAttributesRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            with_data: Arbitrary::arbitrary(g),
            with_parents: Arbitrary::arbitrary(g),
            with_file_metadata: Arbitrary::arbitrary(g),
            with_directory_metadata: Arbitrary::arbitrary(g),
            with_children: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireTreeKeysQuery {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            keys: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireTreeQuery {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        use rand::Rng;
        let variant = g.gen_range(0, 2);
        match variant {
            0 => WireTreeQuery::Other,
            1 => WireTreeQuery::ByKeys(Arbitrary::arbitrary(g)),
            _ => unreachable!(),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireTreeRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            query: Arbitrary::arbitrary(g),
            attributes: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::wire::tests::{check_serialize_roundtrip, check_wire_roundtrip};

    use quickcheck::quickcheck;

    quickcheck! {
        // Wire serialize roundtrips
        fn test_keys_query_roundtrip_serialize(v: WireTreeKeysQuery) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_query_roundtrip_serialize(v: WireTreeQuery) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_attrs_roundtrip_serialize(v: WireTreeAttributesRequest) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_request_roundtrip_serialize(v: WireTreeRequest) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_entry_roundtrip_serialize(v: WireTreeEntry) -> bool {
            check_serialize_roundtrip(v)
        }

        // API-Wire roundtrips

        fn test_request_roundtrip_wire(v: TreeRequest) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_entry_roundtrip_wire(v: Result<TreeEntry, EdenApiServerError>) -> bool {
            check_wire_roundtrip(v)
        }
    }
}
