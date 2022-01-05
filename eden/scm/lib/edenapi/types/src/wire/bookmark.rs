/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
