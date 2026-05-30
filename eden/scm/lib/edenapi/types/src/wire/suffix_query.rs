/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub use crate::suffix_query::WireSuffixQueryRequest;
pub use crate::suffix_query::WireSuffixQueryResponse;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::tests::wire_json_hashes;

    #[test]
    fn test_wire_json() {
        assert_eq!(
            wire_json_hashes![WireSuffixQueryRequest, WireSuffixQueryResponse,],
            [10314064990398522914, 7714689301404996401]
        );
    }
}
