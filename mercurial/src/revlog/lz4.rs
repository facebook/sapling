// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Support for lz4revlog
use std::ptr;

use lz4::liblz4::{LZ4StreamDecode, LZ4_createStreamDecode, LZ4_decompress_safe_continue,
                  LZ4_freeStreamDecode};

use nom::{self, IResult, le_u32};
use super::parser::{Error, detach_result};

// Wrapper for the lz4 library context
struct Context(*mut LZ4StreamDecode);
impl Context {
    // Allocate a context; fails if allocation fails
    fn new() -> Result<Self, &'static str> {
        let ctx = unsafe { LZ4_createStreamDecode() };
        if ctx.is_null() {
            Err("failed to create LZ4 context")
        } else {
            Ok(Context(ctx))
        }
    }
}

// Make sure C resources for context get freed.
impl Drop for Context {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LZ4_freeStreamDecode(self.0) };
            self.0 = ptr::null_mut();
        }
    }
}

// Decompress a raw lz4 block
fn lz4_decompress_block(i: &[u8], out: &mut Vec<u8>) -> Result<usize, &'static str> {
    let ctx = Context::new()?;
    unsafe {
        let ret = LZ4_decompress_safe_continue(
            ctx.0,
            i.as_ptr(),
            out.as_mut_ptr(),
            i.len() as i32,
            out.capacity() as i32,
        );
        if ret < 0 {
            Err("LZ4_decompress_safe_continue failed")
        } else {
            out.set_len(ret as usize);
            Ok(ret as usize)
        }
    }
}

// This is awkward because lz4revlog stores raw unframed lz4 blocks
pub fn lz4_decompress<P, R>(i: &[u8], parse: P) -> IResult<&[u8], R, Error>
where
    for<'a> P: Fn(&'a [u8]) -> IResult<&'a [u8], R, Error> + 'a,
{
    // python lz4 stores original size as le32 at start
    let (i, origsize) = match le_u32(i) {
        IResult::Done(rest, size) => (rest, size),
        err => panic!("getting size err={:?}", err),
    };

    let mut data = Vec::with_capacity(origsize as usize);

    match lz4_decompress_block(i, &mut data) {
        Ok(len) => {
            assert_eq!(origsize as usize, len);
            assert_eq!(origsize as usize, data.len());
        }
        Err(_msg) => return IResult::Error(nom::ErrorKind::Custom(super::parser::Badness::BadLZ4)),
    };

    let inused = i.len();
    let remains = &i[inused..];

    detach_result(parse(&data[..]), remains)
}
