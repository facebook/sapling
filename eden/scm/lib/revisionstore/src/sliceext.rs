/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
