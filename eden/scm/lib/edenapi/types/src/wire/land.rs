/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub use crate::land::WireLandStackRequest;
pub use crate::land::WireLandStackResponse;
pub use crate::land::WirePushVar;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::tests::wire_json_hashes;

    #[test]
    fn test_wire_json() {
        assert_eq!(
            wire_json_hashes![WirePushVar, WireLandStackRequest, WireLandStackResponse,],
            [
                554167251054503582,
                11781303561977194028,
                8680749065747339838
            ]
        );
    }
}
