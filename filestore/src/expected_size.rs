/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use bytes::BytesMut;
use failure_ext::Result;
use std::convert::TryInto;

use crate::errors::ErrorKind;
use crate::incremental_hash::AdvisorySize;

/// ExpectedSize is an opaque struct that lets us encapsulate the incoming size for a Filestore
/// operation and make meaningful comparisons with it, but doesn't let us easily read it back. This
/// ensures that we cannot accidentally use the size hint (we shouldn't be trusted) when we need an
/// actual observed size.
#[derive(Debug, Copy, Clone)]
pub struct ExpectedSize(u64);

impl ExpectedSize {
    pub fn new(size: u64) -> Self {
        Self(size)
    }

    pub fn should_chunk(&self, chunk_size: u64) -> bool {
        self.0 > chunk_size
    }

    pub fn check_equals(&self, size: u64) -> Result<()> {
        if size == self.0 {
            return Ok(());
        }
        Err(ErrorKind::InvalidSize(self.clone(), size).into())
    }

    pub fn check_less(&self, size: u64) -> Result<()> {
        if size <= self.0 {
            return Ok(());
        }
        Err(ErrorKind::InvalidSize(self.clone(), size).into())
    }

    pub fn new_buffer(&self) -> BytesMut {
        // NOTE: This will panic if we can't fit an u64 into usize. That's expected.
        BytesMut::with_capacity(self.0.try_into().unwrap())
    }
}

/// The incremental_hash crate does need access to the internal u64 to create its hashes, so we
/// expose it through a Trait defined here. Don't use that trait if you're not incremental_hash.
impl AdvisorySize for ExpectedSize {
    fn advise(&self) -> u64 {
        self.0
    }
}
