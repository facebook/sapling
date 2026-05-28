/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub use crate::bookmark::WireBookmarkEntry;
pub use crate::bookmark::WireBookmarkRequest;
pub use crate::bookmark::WireSetBookmarkRequest;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::tests::wire_json_hashes;

    #[test]
    fn test_wire_json() {
        assert_eq!(
            wire_json_hashes![
                WireBookmarkRequest,
                WireBookmarkEntry,
                WireSetBookmarkRequest,
            ],
            [
                14914298698918429169,
                6294060359445534470,
                3085324382544079874,
            ]
        );
    }
}
