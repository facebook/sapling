/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::IsTty;
use std::io::Cursor;
use std::sync::Weak;

impl IsTty for std::io::Stdin {
    fn is_tty(&self) -> bool {
        atty::is(atty::Stream::Stdin)
    }
}

impl IsTty for std::io::Stdout {
    fn is_tty(&self) -> bool {
        atty::is(atty::Stream::Stdout)
    }
}

impl IsTty for std::io::Stderr {
    fn is_tty(&self) -> bool {
        atty::is(atty::Stream::Stderr)
    }
}

impl IsTty for Vec<u8> {
    fn is_tty(&self) -> bool {
        false
    }
}

impl<'a> IsTty for &'a [u8] {
    fn is_tty(&self) -> bool {
        false
    }
}

impl<T> IsTty for Cursor<T> {
    fn is_tty(&self) -> bool {
        false
    }
}

impl IsTty for crate::IOOutput {
    fn is_tty(&self) -> bool {
        let inner = match Weak::upgrade(&self.0) {
            Some(inner) => inner,
            None => return false,
        };
        let inner = inner.lock();
        inner.output.is_tty()
    }
}

impl IsTty for crate::IOError {
    fn is_tty(&self) -> bool {
        let inner = match Weak::upgrade(&self.0) {
            Some(inner) => inner,
            None => return false,
        };
        let inner = inner.lock();
        if let Some(error) = inner.error.as_ref() {
            error.is_tty()
        } else {
            false
        }
    }
}

pub(crate) struct PipeWriterWithTty {
    inner: pipe::PipeWriter,
    is_tty: bool,
}

impl std::io::Write for PipeWriterWithTty {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl IsTty for PipeWriterWithTty {
    fn is_tty(&self) -> bool {
        self.is_tty
    }
}

impl PipeWriterWithTty {
    pub fn new(inner: pipe::PipeWriter, is_tty: bool) -> Self {
        Self { inner, is_tty }
    }
}
