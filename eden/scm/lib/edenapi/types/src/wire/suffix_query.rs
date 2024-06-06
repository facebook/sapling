/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub use crate::suffix_query::WireSuffixQueryRequest;
pub use crate::suffix_query::WireSuffixQueryResponse;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(WireSuffixQueryRequest, WireSuffixQueryResponse);
}
