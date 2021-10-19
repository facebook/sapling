/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::token::UploadTokenMetadata;
use crate::wire::ToApi;
use crate::wire::ToWire;
use crate::wire::WireToApiConversionError;
use serde::Deserialize;
use serde::Serialize;

pub use crate::token::WireFileContentTokenMetadata;
pub use crate::token::WireUploadToken;
pub use crate::token::WireUploadTokenData;
pub use crate::token::WireUploadTokenSignature;

/// Token metadata. Could be different for different token types.
/// A signed token guarantee the metadata has been verified.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireUploadTokenMetadata {
    #[serde(rename = "1")]
    WireFileContentTokenMetadata(WireFileContentTokenMetadata),
}

impl ToWire for UploadTokenMetadata {
    type Wire = WireUploadTokenMetadata;

    fn to_wire(self) -> Self::Wire {
        use UploadTokenMetadata::*;
        match self {
            FileContentTokenMetadata(meta) => {
                WireUploadTokenMetadata::WireFileContentTokenMetadata(meta.to_wire())
            }
        }
    }
}

impl ToApi for WireUploadTokenMetadata {
    type Api = UploadTokenMetadata;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        use WireUploadTokenMetadata::*;
        Ok(match self {
            WireFileContentTokenMetadata(meta) => {
                UploadTokenMetadata::FileContentTokenMetadata(meta.to_api()?)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(WireUploadToken);
}
