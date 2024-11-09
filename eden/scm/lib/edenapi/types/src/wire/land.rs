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
    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(WirePushVar, WireLandStackRequest, WireLandStackResponse,);
}
