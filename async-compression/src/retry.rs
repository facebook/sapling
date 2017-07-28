// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! wraps underlying AsyncWrite providing retry logic

use std::io;
use tokio_io::AsyncWrite;

#[inline]
pub fn retry_write<T: AsyncWrite>(writer: &mut T, buf: &[u8]) -> io::Result<usize> {
    // tokio-io doesn't handle EINTR well at the moment, so retry here. See
    // https://github.com/tokio-rs/tokio-io/issues/37 for some discussion.
    loop {
        match writer.write(buf) {
            Ok(n) => return Ok(n),
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}
