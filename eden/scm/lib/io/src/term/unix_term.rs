/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::os::unix::prelude::AsRawFd;

use termwiz::render::RenderTty;

pub(crate) trait RawWrite: io::Write + AsRawFd + Send + Sync {}

impl RawWrite for io::Stderr {}

/// UnixTty is a shim tty to avoid using the real termwiz
/// UnixTerminal for now.
pub(crate) struct UnixTty {
    write: Box<dyn RawWrite>,
}

impl UnixTty {
    pub fn new(write: Box<dyn RawWrite>) -> Self {
        Self { write }
    }
}

impl RenderTty for UnixTty {
    fn get_size_in_cells(&mut self) -> termwiz::Result<(usize, usize)> {
        match terminal_size::terminal_size_using_fd(self.write.as_raw_fd()) {
            Some((width, height)) => Ok((width.0 as _, height.0 as _)),
            // Fallback size, just in case.
            None => Ok((super::DEFAULT_TERM_WIDTH, super::DEFAULT_TERM_HEIGHT)),
        }
    }
}

impl io::Write for UnixTty {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.write.flush()
    }
}
