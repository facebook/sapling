/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{fmt::Debug, slice::SliceIndex};

use failure::{Fail, Fallible as Result};

#[derive(Debug, Fail)]
#[fail(display = "SliceOutOfBounds Error: {:?}", _0)]
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
