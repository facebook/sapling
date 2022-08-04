/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroUsize;

use anyhow::Result;
use fbinit::FacebookInit;

use crate::delay::BlobDelay;

pub fn sharded(
    _fb: FacebookInit,
    _shardmap: String,
    shard_count: NonZeroUsize,
) -> Result<BlobDelay> {
    Ok(BlobDelay::dummy(shard_count))
}

pub fn single(_fb: FacebookInit, _shard: String) -> Result<BlobDelay> {
    Ok(BlobDelay::dummy(
        NonZeroUsize::new(1).expect("1 should never be zero"),
    ))
}
