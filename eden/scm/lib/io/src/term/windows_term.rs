/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io;
use std::os::windows::io::AsRawHandle;
use std::os::windows::prelude::BorrowedHandle;

use termwiz::render::RenderTty;

pub(crate) trait RawWrite: io::Write + AsRawHandle + Send + Sync {}

impl RawWrite for io::Stderr {}

/// WindowsTty is a shim tty to avoid using the real termwiz
/// WindowsTerminal for now. The WindowsTerminal disables automatic
/// "\n" -> "\r\n" conversion, so it messes up our normal newline
/// delimited output that goes to stderr/stdout.
pub(crate) struct WindowsTty {
    write: Box<dyn RawWrite>,
}

impl WindowsTty {
    pub fn new(write: Box<dyn RawWrite>) -> Self {
        Self { write }
    }
}

impl RenderTty for WindowsTty {
    fn get_size_in_cells(&mut self) -> termwiz::Result<(usize, usize)> {
        let handle = unsafe { BorrowedHandle::borrow_raw(self.write.as_raw_handle()) };
        match terminal_size::terminal_size_of(handle) {
            Some((width, height)) => Ok((width.0 as _, height.0 as _)),
            // Fallback size, just in case.
            None => Ok((super::DEFAULT_TERM_WIDTH, super::DEFAULT_TERM_HEIGHT)),
        }
    }
}

impl super::ResettableTty for WindowsTty {}

impl io::Write for WindowsTty {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.write.flush()
    }
}
