// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! VLQ (Variable-length quantity) encoding.

#[cfg(test)]
#[macro_use]
extern crate quickcheck;

use std::io::{self, Read, Write};
use std::mem::size_of;

pub trait VLQEncode<T> {
    /// Encode an integer to a VLQ byte array and write it directly to a stream.
    ///
    /// # Examples
    ///
    /// ```
    /// use vlqencoding::VLQEncode;
    /// let mut v = vec![];
    ///
    /// let x = 120u8;
    /// v.write_vlq(x).expect("writing an encoded u8 to a vec should work");
    /// assert_eq!(v, vec![120]);
    ///
    /// let x = 22742734291u64;
    /// v.write_vlq(x).expect("writing an encoded u64 to a vec should work");
    ///
    /// assert_eq!(v, vec![120, 211, 171, 202, 220, 84]);
    /// ```
    ///
    /// Signed integers are encoded via zig-zag:
    ///
    /// ```
    /// use vlqencoding::VLQEncode;
    /// let mut v = vec![];
    ///
    /// let x = -3i8;
    /// v.write_vlq(x).expect("writing an encoded i8 to a vec should work");
    /// assert_eq!(v, vec![5]);
    ///
    /// let x = 1000i16;
    /// v.write_vlq(x).expect("writing an encoded i16 to a vec should work");
    /// assert_eq!(v, vec![5, 208, 15]);
    /// ```
    fn write_vlq(&mut self, value: T) -> io::Result<()>;
}

pub trait VLQDecode<T> {
    /// Read a VLQ byte array from stream and decode it to an integer.
    ///
    /// # Examples
    ///
    /// ```
    /// use vlqencoding::VLQDecode;
    /// use std::io::{Cursor,Seek,SeekFrom,ErrorKind};
    ///
    /// let mut c = Cursor::new(vec![120u8, 211, 171, 202, 220, 84]);
    ///
    /// let x: Result<u8, _> = c.read_vlq();
    /// assert_eq!(x.unwrap(), 120u8);
    ///
    /// let x: Result<u16, _> = c.read_vlq();
    /// assert_eq!(x.unwrap_err().kind(), ErrorKind::InvalidData);
    ///
    /// c.seek(SeekFrom::Start(1)).expect("seek should work");
    /// let x: Result<u64, _> = c.read_vlq();
    /// assert_eq!(x.unwrap(), 22742734291u64);
    /// ```
    ///
    /// Signed integers are decoded via zig-zag:
    ///
    /// ```
    /// use vlqencoding::VLQDecode;
    /// use std::io::{Cursor,Seek,SeekFrom,ErrorKind};
    ///
    /// let mut c = Cursor::new(vec![5u8, 208, 15]);
    ///
    /// let x: Result<i8, _> = c.read_vlq();
    /// assert_eq!(x.unwrap(), -3i8);
    ///
    /// let x: Result<i8, _> = c.read_vlq();
    /// assert_eq!(x.unwrap_err().kind(), ErrorKind::InvalidData);
    ///
    /// c.seek(SeekFrom::Start(1)).expect("seek should work");
    /// let x: Result<i32, _> = c.read_vlq();
    /// assert_eq!(x.unwrap(), 1000i32);
    /// ```
    fn read_vlq(&mut self) -> io::Result<T>;
}

pub trait VLQDecodeAt<T> {
    /// Read a VLQ byte array from the given offset and decode it to an integer.
    ///
    /// Returns `Ok((decoded_integer, bytes_read))` on success.
    ///
    /// This is similar to `VLQDecode::read_vlq`. It's for immutable `AsRef<[u8]>` instead of
    /// a mutable `io::Read` object.
    ///
    /// # Examples
    ///
    /// ```
    /// use vlqencoding::VLQDecodeAt;
    /// use std::io::ErrorKind;
    ///
    /// let c = &[120u8, 211, 171, 202, 220, 84, 255];
    ///
    /// let x: Result<(u8, _), _> = c.read_vlq_at(0);
    /// assert_eq!(x.unwrap(), (120u8, 1));
    ///
    /// let x: Result<(u64, _), _> = c.read_vlq_at(1);
    /// assert_eq!(x.unwrap(), (22742734291u64, 5));
    ///
    /// let x: Result<(u64, _), _> = c.read_vlq_at(6);
    /// assert_eq!(x.unwrap_err().kind(), ::std::io::ErrorKind::InvalidData);
    ///
    /// let x: Result<(u64, _), _> = c.read_vlq_at(7);
    /// assert_eq!(x.unwrap_err().kind(), ::std::io::ErrorKind::InvalidData);
    /// ```
    fn read_vlq_at(&self, offset: usize) -> io::Result<(T, usize)>;
}

macro_rules! impl_unsigned_primitive {
    ($T: ident) => {
        impl<W: Write + ?Sized> VLQEncode<$T> for W {
            fn write_vlq(&mut self, value: $T) -> io::Result<()> {
                let mut buf = [0u8];
                let mut value = value;
                loop {
                    let mut byte = (value & 127) as u8;
                    let next = value >> 7;
                    if next != 0 {
                        byte |= 128;
                    }
                    buf[0] = byte;
                    self.write_all(&buf)?;
                    value = next;
                    if value == 0 {
                        break;
                    }
                }
                Ok(())
            }
        }

        impl<R: Read + ?Sized> VLQDecode<$T> for R {
            fn read_vlq(&mut self) -> io::Result<$T> {
                let mut buf = [0u8];
                let mut value = 0 as $T;
                let mut base = 1 as $T;
                let base_multiplier = (1 << 7) as $T;
                loop {
                    self.read_exact(&mut buf)?;
                    let byte = buf[0];
                    value = ($T::from(byte & 127))
                        .checked_mul(base)
                        .and_then(|v| v.checked_add(value))
                        .ok_or(io::ErrorKind::InvalidData)?;
                    if byte & 128 == 0 {
                        break;
                    }
                    base = base
                        .checked_mul(base_multiplier)
                        .ok_or(io::ErrorKind::InvalidData)?;
                }
                Ok(value)
            }
        }

        impl<R: AsRef<[u8]>> VLQDecodeAt<$T> for R {
            fn read_vlq_at(&self, offset: usize) -> io::Result<($T, usize)> {
                let buf = self.as_ref();
                let mut size = 0;
                let mut value = 0 as $T;
                let mut base = 1 as $T;
                let base_multiplier = (1 << 7) as $T;
                loop {
                    if let Some(byte) = buf.get(offset + size) {
                        size += 1;
                        value = ($T::from(byte & 127))
                            .checked_mul(base)
                            .and_then(|v| v.checked_add(value))
                            .ok_or(io::ErrorKind::InvalidData)?;
                        if byte & 128 == 0 {
                            break;
                        }
                        base = base
                            .checked_mul(base_multiplier)
                            .ok_or(io::ErrorKind::InvalidData)?;
                    } else {
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                }
                Ok((value, size))
            }
        }
    };
}

impl_unsigned_primitive!(usize);
impl_unsigned_primitive!(u64);
impl_unsigned_primitive!(u32);
impl_unsigned_primitive!(u16);
impl_unsigned_primitive!(u8);

macro_rules! impl_signed_primitive {
    ($T: ty, $U: ty) => {
        impl<W: Write + ?Sized> VLQEncode<$T> for W {
            fn write_vlq(&mut self, v: $T) -> io::Result<()> {
                self.write_vlq(((v << 1) ^ (v >> (size_of::<$U>() * 8 - 1))) as $U)
            }
        }

        impl<R: Read + ?Sized> VLQDecode<$T> for R {
            fn read_vlq(&mut self) -> io::Result<$T> {
                (self.read_vlq() as Result<$U, _>).map(|n| ((n >> 1) as $T) ^ -((n & 1) as $T))
            }
        }

        impl<R: AsRef<[u8]>> VLQDecodeAt<$T> for R {
            fn read_vlq_at(&self, offset: usize) -> io::Result<($T, usize)> {
                (self.read_vlq_at(offset) as Result<($U, _), _>)
                    .map(|(n, s)| (((n >> 1) as $T) ^ -((n & 1) as $T), s))
            }
        }
    };
}

impl_signed_primitive!(isize, usize);
impl_signed_primitive!(i64, u64);
impl_signed_primitive!(i32, u32);
impl_signed_primitive!(i16, u16);
impl_signed_primitive!(i8, u8);

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor, Seek, SeekFrom};

    macro_rules! check_round_trip {
        ($N: expr) => {{
            let mut v = vec![];
            let mut x = $N;
            v.write_vlq(x).expect("write");

            // `z` and `y` below are helpful for the compiler to figure out the return type of
            // `read_vlq_at`, and `read_vlq`.
            #[allow(unused_assignments)]
            let mut z = x;
            let t = v.read_vlq_at(0).unwrap();
            z = t.0;

            let mut c = Cursor::new(v);
            let y = x;
            x = c.read_vlq().unwrap();
            x == y && y == z && t.1 == c.position() as usize
        }};
    }

    #[test]
    fn test_round_trip_manual() {
        for i in (0..64)
            .flat_map(|b| vec![1u64 << b, (1 << b) + 1, (1 << b) - 1].into_iter())
            .chain(vec![0xb3a73ce2ff2, 0xab54a98ceb1f0ad2].into_iter())
            .flat_map(|i| vec![i, !i].into_iter())
        {
            assert!(check_round_trip!(i as i8));
            assert!(check_round_trip!(i as i16));
            assert!(check_round_trip!(i as i32));
            assert!(check_round_trip!(i as i64));
            assert!(check_round_trip!(i as isize));
            assert!(check_round_trip!(i as u8));
            assert!(check_round_trip!(i as u16));
            assert!(check_round_trip!(i as u32));
            assert!(check_round_trip!(i as u64));
            assert!(check_round_trip!(i as usize));
        }
    }

    #[test]
    fn test_read_errors() {
        let mut c = Cursor::new(vec![]);
        assert_eq!(
            (c.read_vlq() as io::Result<u64>).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );

        let mut c = Cursor::new(vec![255, 129]);
        assert_eq!(
            (c.read_vlq() as io::Result<u64>).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );

        c.seek(SeekFrom::Start(0)).unwrap();
        assert_eq!(
            (c.read_vlq() as io::Result<u8>).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn test_zig_zag() {
        let mut c = Cursor::new(vec![]);
        for &(i, u) in [
            (0, 0),
            (-1, 1),
            (1, 2),
            (-2, 3),
            (-127, 253),
            (127, 254),
            (-128i8, 255u8),
        ]
        .iter()
        {
            c.seek(SeekFrom::Start(0)).expect("seek");
            c.write_vlq(i).expect("write");
            c.seek(SeekFrom::Start(0)).expect("seek");
            let x: u8 = c.read_vlq().unwrap();
            assert_eq!(x, u);
        }
    }

    quickcheck! {
        fn test_round_trip_u64_quickcheck(x: u64) -> bool {
            check_round_trip!(x)
        }

        fn test_round_trip_i64_quickcheck(x: i64) -> bool {
            check_round_trip!(x)
        }

        fn test_round_trip_u8_quickcheck(x: u8) -> bool {
            check_round_trip!(x)
        }

        fn test_round_trip_i8_quickcheck(x: i8) -> bool {
            check_round_trip!(x)
        }
    }
}
