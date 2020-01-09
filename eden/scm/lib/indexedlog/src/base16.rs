/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Base16 iterator.

/// Iterating through base16 bytes (0 to 15).
#[derive(Debug, Copy, Clone)]
pub struct Base16Iter<'a, T: 'a>(&'a T, usize, usize);

impl<'a, T: AsRef<[u8]>> Base16Iter<'a, T> {
    /// Convert base256 binary sequence to a base16 iterator.
    pub fn from_base256(binary: &'a T) -> Self {
        let len = binary.as_ref().len() * 2;
        Base16Iter(binary, 0, len)
    }
}

/// Base16 iterator for `[u8]`
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
        let end = self.2.min(self.1 + n);
        Base16Iter(self.0, self.1, end)
    }
}

/// Convert base16 to base256. base16 must have 2 * N items.
///
/// Panic if base16 has 2 * N + 1 items.
pub(crate) fn base16_to_base256(base16: &[u8]) -> Vec<u8> {
    assert!(base16.len() & 1 == 0);
    let mut bytes = Vec::with_capacity(base16.len() / 2);
    let mut next_byte: u8 = 0;
    for (i, b16) in base16.iter().cloned().enumerate() {
        if i & 1 == 0 {
            next_byte = b16 << 4;
        } else {
            bytes.push(next_byte | b16);
        }
    }
    bytes
}

/// Convert a single hex digit to base16 value.
///
/// Return 16 if the digit is invalid.
#[inline]
pub(crate) fn single_hex_to_base16(ch: u8) -> u8 {
    // Does not depend on ASCII order (ex. b'1' - b'0' == 1).
    // Compiler can turn this into a lookup table. It is fast when cached.
    match ch {
        b'0' => 0,
        b'1' => 1,
        b'2' => 2,
        b'3' => 3,
        b'4' => 4,
        b'5' => 5,
        b'6' => 6,
        b'7' => 7,
        b'8' => 8,
        b'9' => 9,
        b'a' | b'A' => 10,
        b'b' | b'B' => 11,
        b'c' | b'C' => 12,
        b'd' | b'D' => 13,
        b'e' | b'E' => 14,
        b'f' | b'F' => 15,
        _ => 16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn check_skip_rev(src: Vec<u8>) -> bool {
            let iter = Base16Iter::from_base256(&src);
            let full: Vec<u8> = iter.clone().collect();
            let rev: Vec<u8> = iter.clone().rev().collect();
            (0..full.len()).all(|i| {
                let v: Vec<u8> = iter.clone().skip(i).collect();
                let r: Vec<u8> = iter.clone().skip(i).rev().rev().rev().collect();
                v.capacity() == v.len() && v[..] == full[i..] &&
                    r.capacity() == r.len() && r[..] == rev[..(rev.len() - i)]
            })
        }

        fn check_roundtrip(src: Vec<u8>) -> bool {
            let iter = Base16Iter::from_base256(&src);
            base16_to_base256(&iter.collect::<Vec<u8>>()) == src
        }
    }

    // The below patterns (skip, zip, rev; skip, take, rev) are used in radix.rs.
    // Make sure they work at iterator level without needing an extra container.

    #[test]
    fn test_zip_skip_rev() {
        let x = [0x12, 0x34, 0x56, 0x21u8];
        let y = [0x78, 0x90, 0xab, 0xcdu8];
        let i = Base16Iter::from_base256(&x)
            .skip(2)
            .zip(Base16Iter::from_base256(&y).skip(3))
            .rev(); // .rev() works directly
        let v: Vec<(u8, u8)> = i.collect();
        assert_eq!(v.capacity(), v.len());
        assert_eq!(v, vec![(2, 0xd), (6, 0xc), (5, 0xb), (4, 0xa), (3, 0)]);
    }

    #[test]
    fn test_skip_take_rev() {
        let x = [0x12, 0x34, 0x56u8];
        let i = Base16Iter::from_base256(&x).skip(3).take(3).rev(); // .rev() works directly
        let v: Vec<u8> = i.collect();
        assert_eq!(v.capacity(), v.len());
        assert_eq!(v, vec![6, 5, 4]);
    }
}
