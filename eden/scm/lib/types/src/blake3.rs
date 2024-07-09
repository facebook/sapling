/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::hash::AbstractHashType;
use crate::hash::HashTypeInfo;

// Define a Blake3 Hash

pub type Blake3 = AbstractHashType<Blake3TypeInfo, 32>;
pub struct Blake3TypeInfo;
impl HashTypeInfo for Blake3TypeInfo {
    const HASH_TYPE_NAME: &'static str = "Blake3";
}
