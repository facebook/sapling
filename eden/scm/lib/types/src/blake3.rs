/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::hash::AbstractHashType;
use crate::hash::HashTypeInfo;

// Define a Blake3 Hash

pub type Blake3 = AbstractHashType<Blake3TypeInfo, 32>;
pub struct Blake3TypeInfo;
impl HashTypeInfo for Blake3TypeInfo {
    const HASH_TYPE_NAME: &'static str = "Blake3";
}
