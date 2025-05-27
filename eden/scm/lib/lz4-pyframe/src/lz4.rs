/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Cursor;

use byteorder::LittleEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use libc::c_int;
use libc::c_void;
use lz4_sys::LZ4_compress_continue;
use lz4_sys::LZ4_compressBound;
use lz4_sys::LZ4_createStream;
use lz4_sys::LZ4_createStreamDecode;
use lz4_sys::LZ4_decompress_safe_continue;
use lz4_sys::LZ4_freeStream;
use lz4_sys::LZ4_freeStreamDecode;
use lz4_sys::LZ4StreamDecode;
use lz4_sys::LZ4StreamEncode;
use thiserror::Error;

use crate::Result;

const HEADER_LEN: usize = 4;

// These function should be exported by lz4-sys. For now, we just declare them.
// See https://github.com/lz4/lz4/blob/dev/lib/lz4hc.h
//
// int LZ4_compress_HC_continue (LZ4_streamHC_t* streamHCPtr, const char* src, char* dst, int
//     srcSize, int maxDstSize);
// LZ4_streamHC_t* LZ4_createStreamHC(void);
// int LZ4_freeStreamHC (LZ4_streamHC_t* streamHCPtr);
#[repr(C)]
struct LZ4_streamHC_t(c_void);
unsafe extern "C" {
    fn LZ4_compress_HC_continue(
        LZ4_stream: *mut LZ4_streamHC_t,
        src: *const u8,
        dst: *mut u8,
        srcSize: c_int,
        maxDstSize: c_int,
    ) -> c_int;

    fn LZ4_createStreamHC() -> *mut LZ4_streamHC_t;
    fn LZ4_freeStreamHC(streamHCPtr: *mut LZ4_streamHC_t) -> c_int;
}

#[derive(Debug, Error)]
pub enum LZ4Error {
    #[error("{message:?}")]
    Generic { message: String },

    #[error("{source:?}")]
    Decompression {
        #[from]
        source: LZ4DecompressionError,
    },

    #[error("{source:?}")]
    Io {
        #[from]
        source: std::io::Error,
    },
}

#[derive(Error, Debug)]
#[error(
    "lz4 decompressed data does not match expected length. \
     Expected '{expected:?}' vs Actual '{actual:?}'"
)]
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
pub fn decompress_size(data: &[u8]) -> Result<usize> {
    if data.is_empty() {
        Ok(0)
    } else {
        let mut cur = Cursor::new(data);
        let max_decompressed_size = cur.read_u32::<LittleEndian>()? as usize;
        Ok(max_decompressed_size)
    }
}

struct StreamEncoderHC(pub *mut LZ4_streamHC_t);
impl StreamEncoderHC {
    fn new() -> Self {
        StreamEncoderHC(unsafe { LZ4_createStreamHC() })
    }
}
impl Drop for StreamEncoderHC {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let error = unsafe { LZ4_freeStreamHC(self.0) };
            if error != 0 {
                panic!("unable to free stream encoder");
            }
        }
    }
}

/// Decompress into a preallocated buffer. The size of `dest` must
/// match what [decompress_size] returns.
pub fn decompress_into(data: &[u8], dest: &mut [u8]) -> Result<()> {
    let stream = StreamDecoder(unsafe { LZ4_createStreamDecode() });
    if stream.0.is_null() {
        return Err(LZ4Error::Generic {
            message: "Unable to construct lz4 stream decoder".to_string(),
        });
    }
    if !dest.is_empty() {
        let data = &data[HEADER_LEN..];
        let source = data.as_ptr();
        let read: i32 = check_error(unsafe {
            LZ4_decompress_safe_continue(
                stream.0,
                source,
                dest.as_mut_ptr(),
                data.len() as i32,
                dest.len() as i32,
            )
        })?;
        if read != dest.len() as i32 {
            return Err(LZ4DecompressionError {
                expected: dest.len(),
                actual: read as usize,
            }
            .into());
        }
    }
    Ok(())
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    let max_decompressed_size = decompress_size(data)?;
    if max_decompressed_size == 0 {
        return Ok(Vec::new());
    }
    let mut dest = vec![0; max_decompressed_size];
    decompress_into(data, &mut dest)?;
    Ok(dest)
}

pub fn compress(data: &[u8]) -> Result<Vec<u8>> {
    let max_compressed_size = (check_error(unsafe { LZ4_compressBound(data.len() as i32) })?
        + HEADER_LEN as i32) as usize;

    let stream = StreamEncoder(unsafe { LZ4_createStream() });
    if stream.0.is_null() {
        return Err(LZ4Error::Generic {
            message: "unable to construct LZ4 stream encoder".to_string(),
        });
    }

    let source = data.as_ptr();
    let mut dest = Vec::<u8>::with_capacity(max_compressed_size);
    dest.write_u32::<LittleEndian>(data.len() as u32)?;

    if !data.is_empty() {
        unsafe { dest.set_len(max_compressed_size) };
        let written: i32 = check_error(unsafe {
            LZ4_compress_continue(
                stream.0,
                source,
                dest.as_mut_ptr().add(HEADER_LEN),
                data.len() as i32,
            )
        })?;
        if written < dest.len() as i32 {
            dest.truncate(written as usize + HEADER_LEN);
        }
    }
    Ok(dest)
}

pub fn compresshc(data: &[u8]) -> Result<Vec<u8>> {
    let max_compressed_size = (check_error(unsafe { LZ4_compressBound(data.len() as i32) })?
        + HEADER_LEN as i32) as usize;

    let stream = StreamEncoderHC::new();
    if stream.0.is_null() {
        return Err(LZ4Error::Generic {
            message: "unable to construct LZ4 stream encoder".to_string(),
        });
    }

    let source = data.as_ptr();
    let mut dest = Vec::<u8>::with_capacity(max_compressed_size);
    dest.write_u32::<LittleEndian>(data.len() as u32)?;

    if !data.is_empty() {
        unsafe { dest.set_len(max_compressed_size) };
        let written: i32 = check_error(unsafe {
            LZ4_compress_HC_continue(
                stream.0,
                source,
                dest.as_mut_ptr().add(HEADER_LEN),
                data.len() as c_int,
                (max_compressed_size - HEADER_LEN) as c_int,
            )
        })?;
        if written < dest.len() as i32 {
            dest.truncate(written as usize + HEADER_LEN);
        }
    }
    Ok(dest)
}

fn check_error(result: i32) -> Result<i32> {
    if result < 0 {
        return Err(LZ4Error::Generic {
            message: format!("lz4 failed with error '{:?}'", result),
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use quickcheck::quickcheck;

    use super::*;

    fn check_roundtrip<T: AsRef<[u8]>>(data: T) -> (Vec<u8>, bool) {
        let data = data.as_ref();
        let compressed = compress(data).unwrap();
        let compressedhc = compresshc(data).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        let decompressedhc = decompress(&compressedhc).unwrap();
        (
            compressed,
            data[..] == decompressed[..] && data[..] == decompressedhc[..],
        )
    }

    #[test]
    fn test_roundtrip() {
        let data = &b"\x00\x01\x02hello world long string easy easy easy easy compress\xF0\xFA"[..];
        let (compressed, roundtrips) = check_roundtrip(data);
        assert!(compressed.len() < data.len());
        assert!(roundtrips);
    }

    #[test]
    fn test_empty() {
        let data = &b"";
        let (compressed, roundtrips) = check_roundtrip(data);
        assert_eq!(compressed, &[0u8, 0, 0, 0][..]);
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
        assert_eq!(data, &*decompressed);
    }

    quickcheck! {
        fn test_quickcheck_roundtrip(data: Vec<u8>) -> bool {
            check_roundtrip(data).1
        }
    }
}
