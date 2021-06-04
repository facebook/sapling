/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use serde_derive::{Deserialize, Serialize};

use crate::token::{UploadToken, UploadTokenData, UploadTokenSignature};
use crate::wire::{is_default, ToApi, ToWire, WireAnyId, WireToApiConversionError};

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireUploadTokenData {
    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    pub id: WireAnyId,
    // other data to be added ...
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireUploadTokenSignature {
    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    pub signature: Vec<u8>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireUploadToken {
    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    pub data: WireUploadTokenData,
    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    pub signature: WireUploadTokenSignature,
}

impl ToWire for UploadToken {
    type Wire = WireUploadToken;

    fn to_wire(self) -> Self::Wire {
        WireUploadToken {
            data: self.data.to_wire(),
            signature: self.signature.to_wire(),
        }
    }
}

impl ToApi for WireUploadToken {
    type Api = UploadToken;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(UploadToken {
            data: self.data.to_api()?,
            signature: self.signature.to_api()?,
        })
    }
}

impl ToWire for UploadTokenSignature {
    type Wire = WireUploadTokenSignature;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            signature: self.signature.to_wire(),
        }
    }
}

impl ToApi for WireUploadTokenSignature {
    type Api = UploadTokenSignature;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let api = Self::Api {
            signature: self.signature.to_api()?,
        };
        Ok(api)
    }
}

impl ToWire for UploadTokenData {
    type Wire = WireUploadTokenData;

    fn to_wire(self) -> Self::Wire {
        WireUploadTokenData {
            id: self.id.to_wire(),
        }
    }
}

impl ToApi for WireUploadTokenData {
    type Api = UploadTokenData;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(UploadTokenData {
            id: self.id.to_api()?,
        })
    }
}
