/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use hyper::Uri;
use quickcheck::{Arbitrary, Gen};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use mononoke_types::hash::Sha256;

use crate::str_serialized;

// This module provides types conforming to the Git-LFS protocol specification:
// https://github.com/git-lfs/git-lfs/blob/master/docs/api/batch.md

#[derive(Clone, Serialize, Debug, Deserialize, Eq, PartialEq, Hash)]
pub enum Operation {
    #[serde(rename = "download")]
    Download,
    #[serde(rename = "upload")]
    Upload,
}

impl Arbitrary for Operation {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        if bool::arbitrary(g) {
            Operation::Download
        } else {
            Operation::Upload
        }
    }
}

#[derive(Clone, Serialize, Debug, Deserialize, Eq, PartialEq, Hash)]
pub enum Transfer {
    #[serde(rename = "basic")]
    Basic,
    #[serde(other)]
    Unknown,
}

impl Arbitrary for Transfer {
    fn arbitrary<G: Gen>(_g: &mut G) -> Self {
        // We don't generate invalid Transfer instances for testing.
        Transfer::Basic
    }
}

impl Default for Transfer {
    fn default() -> Self {
        Self::Basic
    }
}

#[derive(Clone, Serialize, Debug, Deserialize, Hash, Eq, PartialEq)]
pub struct Ref {
    pub name: String,
}

impl Arbitrary for Ref {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        Self {
            name: String::arbitrary(g),
        }
    }
}

#[derive(Clone, Serialize, Debug, Deserialize, Hash, Eq, PartialEq, Copy)]
pub struct RequestObject {
    #[serde(with = "str_serialized")]
    pub oid: Sha256,
    pub size: u64,
}

impl Arbitrary for RequestObject {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        Self {
            oid: Sha256::arbitrary(g),
            size: u64::arbitrary(g),
        }
    }
}

fn default_client_transfers() -> Vec<Transfer> {
    vec![Transfer::default()]
}

#[derive(Clone, Serialize, Debug, Deserialize, PartialEq)]
pub struct RequestBatch {
    pub operation: Operation,
    #[serde(default = "default_client_transfers")]
    pub transfers: Vec<Transfer>,
    pub r#ref: Option<Ref>,
    pub objects: Vec<RequestObject>,
}

impl Arbitrary for RequestBatch {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        Self {
            operation: Operation::arbitrary(g),
            transfers: Vec::arbitrary(g),
            r#ref: Option::arbitrary(g),
            objects: Vec::arbitrary(g),
        }
    }
}

#[derive(Clone, Serialize, Debug, Deserialize, PartialEq)]
pub struct ObjectAction {
    #[serde(with = "str_serialized")]
    pub href: Uri,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

impl Arbitrary for ObjectAction {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        // We generate a basic URL here. Nothing very fancy.
        let proto = if bool::arbitrary(g) { "http" } else { "https" };

        let domain = if bool::arbitrary(g) {
            "foo.com"
        } else {
            "bar.com"
        };

        let path = if bool::arbitrary(g) { "" } else { "/123" };

        let uri: Uri = format!("{}://{}{}", proto, domain, path).parse().unwrap();

        Self {
            href: uri,
            header: Option::arbitrary(g),
            expires_in: Option::arbitrary(g),
            expires_at: Option::arbitrary(g),
        }
    }
}

impl ObjectAction {
    pub fn new(href: Uri) -> Self {
        Self {
            href,
            header: None,
            expires_in: None,
            expires_at: None,
        }
    }
}

#[derive(Clone, Serialize, Debug, Deserialize, Hash, PartialEq, Eq)]
pub struct ObjectError {
    pub code: u16,
    pub message: String,
}

impl Arbitrary for ObjectError {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        Self {
            code: u16::arbitrary(g),
            message: String::arbitrary(g),
        }
    }
}

#[derive(Clone, Serialize, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ObjectStatus {
    Ok {
        #[serde(default)]
        authenticated: bool,
        actions: HashMap<Operation, ObjectAction>,
    },
    Err {
        error: ObjectError,
    },
}

impl Arbitrary for ObjectStatus {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        if bool::arbitrary(g) {
            let mut actions = HashMap::new();

            if bool::arbitrary(g) {
                actions.insert(Operation::Download, ObjectAction::arbitrary(g));
            }

            if bool::arbitrary(g) {
                actions.insert(Operation::Upload, ObjectAction::arbitrary(g));
            }

            Self::Ok {
                authenticated: bool::arbitrary(g),
                actions,
            }
        } else {
            Self::Err {
                error: ObjectError::arbitrary(g),
            }
        }
    }
}

#[derive(Clone, Serialize, Debug, Deserialize, PartialEq)]
pub struct ResponseObject {
    #[serde(flatten)]
    pub object: RequestObject,
    #[serde(flatten)]
    pub status: ObjectStatus,
}

impl Arbitrary for ResponseObject {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        Self {
            object: RequestObject::arbitrary(g),
            status: ObjectStatus::arbitrary(g),
        }
    }
}

#[derive(Clone, Serialize, Debug, Deserialize, PartialEq)]
pub struct ResponseBatch {
    #[serde(default)]
    pub transfer: Transfer,
    pub objects: Vec<ResponseObject>,
}

impl Arbitrary for ResponseBatch {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        Self {
            transfer: Transfer::arbitrary(g),
            objects: Vec::arbitrary(g),
        }
    }
}

#[derive(Clone, Serialize, Debug, Deserialize)]
pub struct ResponseError {
    pub message: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub documentation_url: Option<Uri>,
    pub request_id: Option<String>,
}

impl Arbitrary for ResponseError {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        Self {
            message: String::arbitrary(g),
            // TODO: It'd be nice to generate those too.
            documentation_url: None,
            request_id: Option::arbitrary(g),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use assert_matches::assert_matches;
    use maplit::hashmap;
    use quickcheck::quickcheck;
    use serde_json::{self, json};

    const ONES_SHA256: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    #[test]
    pub fn test_deserialize_ok_object() {
        let j = json!({
            "oid": ONES_SHA256,
            "size": 123,
            "actions": {
                "download": {
                    "href": "https://some-download.com",
                    "header": {
                        "Key": "value"
                    },
                    "expires_at": "2016-11-10T15:29:07Z",
                }
            }
        });

        assert_matches!(
            serde_json::from_str::<ResponseObject>(&j.to_string()),
            Ok(ResponseObject {
                object: RequestObject { oid: _, size: 123 },
                status:
                    ObjectStatus::Ok {
                        authenticated: false,
                        actions: _,
                    },
            })
        )
    }

    #[test]
    pub fn test_deserialize_err_object() {
        let j = json!({
            "oid": ONES_SHA256,
            "size": 123,
            "error": {
                "code": 404,
                "message": "Object does not exist"
            }
        });

        assert_matches!(
            serde_json::from_str::<ResponseObject>(&j.to_string()),
            Ok(ResponseObject {
                object: RequestObject { oid: _, size: 123 },
                status:
                    ObjectStatus::Err {
                        error:
                            ObjectError {
                                code: 404,
                                message: _,
                            },
                    },
            })
        )
    }

    #[test]
    pub fn test_deserialize_action() {
        let j = json!({
            "href": "https://some-download.com",
            "header": {
                "Key": "value"
            },
            "expires_at": "2016-11-10T15:29:07Z",
        });

        let res = serde_json::from_str::<ObjectAction>(&j.to_string()).unwrap();
        assert_eq!(
            res.href,
            "https://some-download.com".parse::<Uri>().unwrap()
        );
        assert_eq!(
            res.header,
            Some(hashmap! { "Key".to_string() => "value".to_string() })
        );
        assert_eq!(res.expires_at, Some("2016-11-10T15:29:07Z".to_string()));
    }

    quickcheck! {
        fn request_batch_roundtrip(batch: RequestBatch) -> bool {
            let json = serde_json::to_string(&batch).unwrap();
            let rt = serde_json::from_str::<RequestBatch>(&json).unwrap();
            rt == batch
        }

        fn response_batch_roundtrip(batch: ResponseBatch) -> bool {
            let json = serde_json::to_string(&batch).unwrap();
            let rt = serde_json::from_str::<ResponseBatch>(&json).unwrap();
            rt == batch
        }
    }
}
