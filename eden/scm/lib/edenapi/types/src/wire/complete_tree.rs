/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub use crate::complete_tree::WireCompleteTreeRequest;

#[cfg(test)]
mod tests {
    use super::*;

    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(WireCompleteTreeRequest);
}
