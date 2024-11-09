/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Analyze tracing data for sapling
//!
//! This is sapling application specific. It's not a general purposed library.

mod tables;
pub use tables::extract_tables;
