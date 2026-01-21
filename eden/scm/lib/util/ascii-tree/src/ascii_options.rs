/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/// Options used to control behavior of writing ASCII graph.
#[derive(Default)]
pub struct AsciiOptions {
    /// Hide a `TreeSpan` if a span takes less than the specified
    /// duration.
    pub min_duration_to_hide: u64,

    /// Hide a `TreeSpan` if it is less than the specified
    /// percentage of the parent's duration (0 to 100).
    pub min_duration_parent_percentage_to_hide: u8,

    /// Show a `TreeSpan` if it was hidden by the above rules
    /// but it is more than the specified percentage of the parent's
    /// duration. Not effective if it's 0.
    pub min_duration_parent_percentage_to_show: u8,
}
