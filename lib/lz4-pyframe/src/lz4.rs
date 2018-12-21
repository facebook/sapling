// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use failure::Error;
use lz4_sys::{LZ4StreamDecode, LZ4StreamEncode, LZ4_compressBound, LZ4_compress_continue,
              LZ4_createStream, LZ4_createStreamDecode, LZ4_decompress_safe_continue,
              LZ4_freeStream, LZ4_freeStreamDecode};
use std::io::Cursor;

const HEADER_LEN: usize = 4;

#[derive(Debug, Fail)]
#[fail(display = "{:?}", message)]
pub struct LZ4Error {
    message: String,
}

#[derive(Fail, Debug)]
#[fail(display = "lz4 decompressed data does not match expected length. \
                  Expected '{:?}' vs Actual '{:?}'",
       expected, actual)]
pub struct LZ4DecompressionError {
    expected: usize,
    actual: usize,
}

struct StreamDecoder(pub *mut LZ4StreamDecode);
impl Drop for StreamDecoder {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let error = unsafe { LZ4_freeStreamDecode(self.0) };
            if error != 0 {
                panic!("unable to free stream decoder");
            }
        }
    }
}

struct StreamEncoder(pub *mut LZ4StreamEncode);
impl Drop for StreamEncoder {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let error = unsafe { LZ4_freeStream(self.0) };
            if error != 0 {
                panic!("unable to free stream encoder");
            }
        }
    }
}

/// Read decompressed size from a u32 header.
pub fn decompress_size(data: &[u8]) -> Result<usize, Error> {
    if data.len() == 0 {
        Ok(0)
    } else {
        let mut cur = Cursor::new(data);
        let max_decompressed_size = cur.read_u32::<LittleEndian>()? as usize;
        Ok(max_decompressed_size)
    }
}

/// Decompress into a preallocated buffer. The size of `dest` must
/// match what [decompress_size] returns.
pub fn decompress_into(data: &[u8], dest: &mut [u8]) -> Result<(), Error> {
    let stream = StreamDecoder(unsafe { LZ4_createStreamDecode() });
    if stream.0.is_null() {
        return Err(LZ4Error {
            message: "Unable to construct lz4 stream decoder".to_string(),
        }.into());
    }
    if dest.len() > 0 {
        let data = &data[HEADER_LEN..];
        let source = data.as_ptr();
        let read: i32 = check_error(unsafe {
            LZ4_decompress_safe_continue(
                stream.0,
                source,
                dest.as_mut_ptr() as *mut u8,
                data.len() as i32,
                dest.len() as i32,
            )
        })?;
        if read != dest.len() as i32 {
            return Err(LZ4DecompressionError {
                expected: dest.len(),
                actual: read as usize,
            }.into());
        }
    }
    Ok(())
}

pub fn decompress(data: &[u8]) -> Result<Box<[u8]>, Error> {
    let max_decompressed_size = decompress_size(data)?;
    if max_decompressed_size == 0 {
        return Ok(Vec::new().into_boxed_slice());
    }
    let mut dest = Vec::<u8>::with_capacity(max_decompressed_size);
    unsafe { dest.set_len(max_decompressed_size) };
    decompress_into(data, &mut dest)?;
    Ok(dest.into_boxed_slice())
}

pub fn compress(data: &[u8]) -> Result<Box<[u8]>, Error> {
    let max_compressed_size = (check_error(unsafe { LZ4_compressBound(data.len() as i32) })?
        + HEADER_LEN as i32) as usize;

    let stream = StreamEncoder(unsafe { LZ4_createStream() });
    if stream.0.is_null() {
        return Err(LZ4Error {
            message: "unable to construct LZ4 stream encoder".to_string(),
        }.into());
    }

    let source = data.as_ptr();
    let mut dest = Vec::<u8>::with_capacity(max_compressed_size);
    dest.write_u32::<LittleEndian>(data.len() as u32)?;

    if data.len() > 0 {
        unsafe { dest.set_len(max_compressed_size) };
        let written: i32 = check_error(unsafe {
            LZ4_compress_continue(
                stream.0,
                source,
                dest.as_mut_ptr().offset(HEADER_LEN as isize),
                data.len() as i32,
            )
        })?;
        if written < dest.len() as i32 {
            dest.truncate(written as usize + HEADER_LEN);
        }
    }
    Ok(dest.into_boxed_slice())
}

fn check_error(result: i32) -> Result<i32, Error> {
    if result < 0 {
        return Err(LZ4Error {
            message: format!("lz4 failed with error '{:?}'", result),
        }.into());
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_roundtrip<T: AsRef<[u8]>>(data: T) -> (Box<[u8]>, bool) {
        let data = data.as_ref();
        let compressed = compress(data).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        (compressed, data[..] == decompressed[..])
    }

    #[test]
    fn test_roundtrip() {
        let data = &b"\x00\x01\x02hello world long string easy easy easy easy compress\xF0\xFA"[..];
        let (compressed, roundtrips) = check_roundtrip(&data);
        assert!(compressed.len() < data.len());
        assert!(roundtrips);
    }

    #[test]
    fn test_empty() {
        let data = &b"";
        let (compressed, roundtrips) = check_roundtrip(&data);
        assert_eq!(compressed, vec![0u8, 0, 0, 0].into_boxed_slice());
        assert!(roundtrips);
    }

    #[test]
    fn test_short() {
        let data = &b"0"[..];
        let compressed = compress(data).unwrap();

        // Short strings should compress to be longer than they used to be.
        assert!(compressed.len() > data.len());

        // But decompress to their original form.
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(data, decompressed.as_ref());
    }

    quickcheck! {
        fn test_quickcheck_roundtrip(data: Vec<u8>) -> bool {
            check_roundtrip(&data).1
        }
    }
}
