/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;

use sha1::Digest;
use sha1::Sha1;
use types::Id20;

#[derive(Default)]
pub(crate) struct Sha1Write(Sha1);

impl io::Write for Sha1Write {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Into<Id20> for Sha1Write {
    fn into(self) -> Id20 {
        Id20::from_byte_array(self.0.finalize().into())
    }
}
