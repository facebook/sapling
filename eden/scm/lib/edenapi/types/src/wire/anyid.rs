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
    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(
        WireAnyId,
        WireLookupRequest,
        WireLookupResponse,
        WireLookupResult,
    );
}
