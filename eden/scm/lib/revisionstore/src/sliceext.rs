/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Debug;
use std::slice::SliceIndex;

use anyhow::Result;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("SliceOutOfBounds Error: {0:?}")]
struct SliceOutOfBoundsError(String);

pub trait SliceExt<'a, T> {
    fn get_err<I>(self, range: I) -> Result<&'a I::Output>
    where
        I: SliceIndex<[T]> + Clone + Debug;
}

impl<'a, T> SliceExt<'a, T> for &'a [T] {
    fn get_err<I>(self, range: I) -> Result<&'a I::Output>
    where
        I: SliceIndex<[T]> + Clone + Debug,
    {
        self.get(range.clone()).ok_or_else(|| {
            SliceOutOfBoundsError(format!(
                "slice (len {:?}) too short to read range {:?}",
                self.len(),
                range
            ))
            .into()
        })
    }
}
