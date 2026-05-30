/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
    use crate::wire::tests::wire_json_hashes;

    #[test]
    fn test_wire_json() {
        assert_eq!(
            wire_json_hashes![WireUploadToken, WireIndexableId,],
            [3507574737796826804, 12507243810918595860]
        );
    }
}
