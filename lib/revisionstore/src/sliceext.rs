// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::ops::Range;

use error::Result;

#[derive(Debug, Fail)]
#[fail(display = "SliceOutOfBounds Error: {:?}", _0)]
struct SliceOutOfBoundsError(String);

pub trait SliceExt<'a, T> {
    fn get_err(self, range: Range<usize>) -> Result<&'a [T]>;
}

impl<'a, T> SliceExt<'a, T> for &'a [T] {
    fn get_err(self, range: Range<usize>) -> Result<&'a [T]> {
        self.get(range.clone()).ok_or_else(|| {
            SliceOutOfBoundsError(format!(
                "slice (len {:?}) too short to read range {:?}",
                self.len(),
                range
            )).into()
        })
    }
}
