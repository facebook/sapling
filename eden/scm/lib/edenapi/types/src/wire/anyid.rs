/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub use crate::anyid::WireAnyId;
pub use crate::anyid::WireLookupRequest;
pub use crate::anyid::WireLookupResponse;
pub use crate::anyid::WireLookupResult;
use crate::commitid::BonsaiChangesetId;
use crate::commitid::GitSha1;
use crate::wire::ToApi;
use crate::wire::ToWire;

wire_hash! {
    wire => WireBonsaiChangesetId,
    api  => BonsaiChangesetId,
    size => 32,
}

wire_hash! {
    wire => WireGitSha1,
    api  => GitSha1,
    size => 20,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::tests::wire_json_hashes;

    #[test]
    fn test_wire_json() {
        assert_eq!(
            wire_json_hashes![
                WireAnyId,
                WireLookupRequest,
                WireLookupResponse,
                WireLookupResult,
            ],
            [
                12336618905119236929,
                12507243810918595860,
                6906579826677505766,
                14052178676412488757
            ]
        );
    }
}
