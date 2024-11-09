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
    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(
        WireBookmarkRequest,
        WireBookmarkEntry,
        WireSetBookmarkRequest
    );
}
