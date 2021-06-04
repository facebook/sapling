/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::AnyId;
use serde_derive::{Deserialize, Serialize};

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct UploadTokenData {
    pub id: AnyId,
    // TODO: add other data (like expiration time).
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct UploadTokenSignature {
    pub signature: Vec<u8>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct UploadToken {
    pub data: UploadTokenData,
    pub signature: UploadTokenSignature,
}

impl UploadToken {
    pub fn new_fake_token(id: AnyId) -> Self {
        Self {
            data: UploadTokenData { id },
            signature: UploadTokenSignature {
                signature: "faketokensignature".into(),
            },
        }
    }
    // TODO: implement secure signed tokens
}
