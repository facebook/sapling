/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Cursor;
use std::sync::Weak;

use crate::IsTty;

impl IsTty for std::io::Stdin {
    fn is_tty(&self) -> bool {
        atty::is(atty::Stream::Stdin)
    }
    fn is_stdin(&self) -> bool {
        true
    }
}

impl IsTty for std::io::Stdout {
    fn is_tty(&self) -> bool {
        atty::is(atty::Stream::Stdout)
    }
    fn is_stdout(&self) -> bool {
        true
    }
}

impl IsTty for std::io::Stderr {
    fn is_tty(&self) -> bool {
        atty::is(atty::Stream::Stderr)
    }
    fn is_stderr(&self) -> bool {
        true
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
    fn is_stdout(&self) -> bool {
        let inner = match Weak::upgrade(&self.0) {
            Some(inner) => inner,
            None => return false,
        };
        let inner = inner.lock();
        inner.output.is_stdout()
    }
    fn pager_active(&self) -> bool {
        let inner = match Weak::upgrade(&self.0) {
            Some(inner) => inner,
            None => return false,
        };
        let inner = inner.lock();
        inner.output.pager_active()
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
    fn is_stderr(&self) -> bool {
        let inner = match Weak::upgrade(&self.0) {
            Some(inner) => inner,
            None => return false,
        };
        let inner = inner.lock();
        if let Some(error) = inner.error.as_ref() {
            error.is_stderr()
        } else {
            false
        }
    }
    fn pager_active(&self) -> bool {
        let inner = match Weak::upgrade(&self.0) {
            Some(inner) => inner,
            None => return false,
        };
        let inner = inner.lock();
        if let Some(error) = inner.error.as_ref() {
            error.pager_active()
        } else {
            false
        }
    }
}

pub(crate) struct PipeWriterWithTty {
    inner: pipe::PipeWriter,
    pretend_tty: bool,
    pub(crate) pretend_stdout: bool,
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
        self.pretend_tty
    }
    fn is_stdout(&self) -> bool {
        self.pretend_stdout
    }
    fn pager_active(&self) -> bool {
        true
    }
}

impl PipeWriterWithTty {
    pub fn new(inner: pipe::PipeWriter, pretend_tty: bool) -> Self {
        Self {
            inner,
            pretend_tty,
            pretend_stdout: false,
        }
    }
}
