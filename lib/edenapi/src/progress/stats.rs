// Copyright Facebook, Inc. 2019

use std::{
    fmt,
    iter::Sum,
    ops::{Add, AddAssign, Sub, SubAssign},
};

#[derive(Default, Debug, Copy, Clone)]
pub struct ProgressStats {
    pub downloaded: usize,
    pub uploaded: usize,
    pub dltotal: usize,
    pub ultotal: usize,
}

impl ProgressStats {
    pub fn new(downloaded: usize, uploaded: usize, dltotal: usize, ultotal: usize) -> Self {
        Self {
            downloaded,
            uploaded,
            dltotal,
            ultotal,
        }
    }

    pub fn as_tuple(&self) -> (usize, usize, usize, usize) {
        (self.downloaded, self.dltotal, self.uploaded, self.ultotal)
    }
}

impl fmt::Display for ProgressStats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Downloaded: {}/{} bytes; Uploaded {}/{} bytes",
            self.downloaded, self.dltotal, self.uploaded, self.ultotal
        )
    }
}

impl Add for ProgressStats {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            downloaded: self.downloaded + other.downloaded,
            uploaded: self.uploaded + other.uploaded,
            dltotal: self.dltotal + other.dltotal,
            ultotal: self.ultotal + other.ultotal,
        }
    }
}

impl AddAssign for ProgressStats {
    fn add_assign(&mut self, other: ProgressStats) {
        *self = *self + other
    }
}

impl Sub for ProgressStats {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            downloaded: self.downloaded - other.downloaded,
            uploaded: self.uploaded - other.uploaded,
            dltotal: self.dltotal - other.dltotal,
            ultotal: self.ultotal - other.ultotal,
        }
    }
}

impl SubAssign for ProgressStats {
    fn sub_assign(&mut self, other: ProgressStats) {
        *self = *self - other
    }
}

impl Sum for ProgressStats {
    fn sum<I: Iterator<Item = ProgressStats>>(iter: I) -> ProgressStats {
        iter.fold(Default::default(), Add::add)
    }
}
