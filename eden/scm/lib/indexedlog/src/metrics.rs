/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(feature = "metrics")]
pub use metrics::Counter;

#[cfg(not(feature = "metrics"))]
pub struct Counter;

#[cfg(not(feature = "metrics"))]
impl Counter {
    pub const fn new_counter(_name: &'static str) -> Self {
        Self
    }

    pub fn increment(&'static self) {
        self.add(1);
    }

    pub fn add(&'static self, _val: usize) {}

    pub fn sub(&'static self, _val: usize) {}

    pub fn value(&'static self) -> usize {
        0
    }
}
