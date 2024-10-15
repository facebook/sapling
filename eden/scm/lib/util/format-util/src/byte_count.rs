/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;

#[derive(Default, Clone, Copy)]
pub(crate) struct ByteCount(usize);

impl io::Write for ByteCount {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = buf.len();
        self.0 += len;
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl From<ByteCount> for usize {
    fn from(value: ByteCount) -> Self {
        value.0
    }
}
