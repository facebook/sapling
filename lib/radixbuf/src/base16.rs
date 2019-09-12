// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Base16 iterator
//!
//! The main radix tree uses base16 to support hex string prefix lookups and
//! make the space usage more efficient.

#[derive(Debug, Copy, Clone)]
pub struct Base16Iter<'a, T>(&'a T, usize, usize);

impl<'a, T: AsRef<[u8]>> Base16Iter<'a, T> {
    /// Convert base256 binary sequence to a base16 iterator.
    pub fn from_bin(binary: &'a T) -> Self {
        let len = binary.as_ref().len() * 2;
        Base16Iter(binary, 0, len)
    }
}

/// Base16 iterator for [u8]
impl<'a, T: AsRef<[u8]>> Iterator for Base16Iter<'a, T> {
    type Item = u8;

    #[inline]
    fn next(&mut self) -> Option<u8> {
        if self.2 <= self.1 {
            None
        } else {
            let i = self.1;
            self.1 = i + 1;
            let v = self.0.as_ref()[i / 2];
            if i & 1 == 0 { v >> 4 } else { v & 0xf }.into()
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }

    #[inline]
    fn count(self) -> usize {
        self.len()
    }
}

impl<'a, T: AsRef<[u8]>> DoubleEndedIterator for Base16Iter<'a, T> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.2 <= self.1 {
            None
        } else {
            let i = self.2 - 1;
            self.2 = i;
            let v = self.0.as_ref()[i / 2];
            if i & 1 == 0 { v >> 4 } else { v & 0xf }.into()
        }
    }
}

impl<'a, T: AsRef<[u8]>> ExactSizeIterator for Base16Iter<'a, T> {
    #[inline]
    fn len(&self) -> usize {
        self.2 - self.1
    }
}

impl<'a, T: AsRef<[u8]>> Base16Iter<'a, T> {
    #[inline]
    pub fn skip(self, n: usize) -> Self {
        Base16Iter(self.0, self.1 + n, self.2)
    }

    #[inline]
    pub fn take(self, n: usize) -> Self {
        // TODO: Use (self.1 + n).min(self.2) once ord_max_min is available (Rust 1.22)
        let mut end = self.1 + n;
        if self.2 < end {
            end = self.2
        }
        Base16Iter(self.0, self.1, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn check_skip_rev(src: Vec<u8>) -> bool {
            let iter = Base16Iter::from_bin(&src);
            let full: Vec<u8> = iter.clone().collect();
            let rev: Vec<u8> = iter.clone().rev().collect();
            (0..full.len()).all(|i| {
                let v: Vec<u8> = iter.clone().skip(i).collect();
                let r: Vec<u8> = iter.clone().skip(i).rev().rev().rev().collect();
                v.capacity() == v.len() && v[..] == full[i..] &&
                    r.capacity() == r.len() && r[..] == rev[..(rev.len() - i)]
            })
        }
    }

    // The below patterns (skip, zip, rev; skip, take, rev) are used in radix.rs.
    // Make sure they work at iterator level without needing an extra container.

    #[test]
    fn test_zip_skip_rev() {
        let x = [0x12, 0x34, 0x56, 0x21u8];
        let y = [0x78, 0x90, 0xab, 0xcdu8];
        let i = Base16Iter::from_bin(&x)
            .skip(2)
            .zip(Base16Iter::from_bin(&y).skip(3))
            .rev(); // .rev() works directly
        let v: Vec<(u8, u8)> = i.collect();
        assert_eq!(v.capacity(), v.len());
        assert_eq!(v, vec![(2, 0xd), (6, 0xc), (5, 0xb), (4, 0xa), (3, 0)]);
    }

    #[test]
    fn test_skip_take_rev() {
        let x = [0x12, 0x34, 0x56u8];
        let i = Base16Iter::from_bin(&x).skip(3).take(3).rev(); // .rev() works directly
        let v: Vec<u8> = i.collect();
        assert_eq!(v.capacity(), v.len());
        assert_eq!(v, vec![6, 5, 4]);
    }

}
