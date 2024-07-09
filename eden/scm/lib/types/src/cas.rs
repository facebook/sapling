/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::Blake3;

#[derive(Debug)]
pub struct CasDigest {
    pub hash: Blake3,
    pub size: u64,
}
