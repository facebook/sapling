/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub(crate) type Counter = stats_traits::stat_types::BoxSingletonCounter;

pub(crate) fn new_counter(name: &'static str) -> Counter {
    stats::create_singleton_counter(name.to_string())
}

pub(crate) fn increment(counter: &Counter, value: i64) {
    if !fbinit::was_performed() {
        return;
    }

    counter.increment_value(fbinit::expect_init(), value);
}
