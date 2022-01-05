/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::anyid::BonsaiChangesetId;
pub use crate::anyid::WireAnyId;
pub use crate::anyid::WireLookupRequest;
pub use crate::anyid::WireLookupResponse;
pub use crate::anyid::WireLookupResult;
use crate::wire::ToApi;
use crate::wire::ToWire;

wire_hash! {
    wire => WireBonsaiChangesetId,
    api  => BonsaiChangesetId,
    size => 32,
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
