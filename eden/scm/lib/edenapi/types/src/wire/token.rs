/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub use crate::token::WireFileContentTokenMetadata;
pub use crate::token::WireIndexableId;
pub use crate::token::WireUploadToken;
pub use crate::token::WireUploadTokenData;
pub use crate::token::WireUploadTokenMetadata;
pub use crate::token::WireUploadTokenSignature;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(WireUploadToken, WireIndexableId);
}
