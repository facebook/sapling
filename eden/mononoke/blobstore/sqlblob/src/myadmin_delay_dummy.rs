/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;

use fbinit::FacebookInit;

use crate::delay::BlobDelay;

pub fn sharded(_fb: FacebookInit, _shardmap: &str) -> Result<BlobDelay> {
    Ok(BlobDelay::dummy())
}

pub fn single(_fb: FacebookInit, _shard: String) -> Result<BlobDelay> {
    Ok(BlobDelay::dummy())
}
