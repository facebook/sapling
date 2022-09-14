/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;

use termwiz::caps::Capabilities;
use termwiz::render::terminfo::TerminfoRenderer;
use termwiz::render::RenderTty;
use termwiz::surface::Change;
use termwiz::terminal::Terminal;
use termwiz::Result;

use crate::IsTty;

pub(crate) const DEFAULT_TERM_WIDTH: usize = 80;
pub(crate) const DEFAULT_TERM_HEIGHT: usize = 25;

#[cfg(windows)]
mod windows_term;

#[cfg(unix)]
mod unix_term;

/// Term is a minimally skinny abstraction over termwiz::Terminal.
/// It makes it easy to swap in other things for testing.
pub(crate) trait Term {
    fn render(&mut self, changes: &[Change]) -> Result<()>;
    fn size(&mut self) -> Result<(usize, usize)>;
}

impl<T: Terminal> Term for T {
    fn render(&mut self, changes: &[Change]) -> Result<()> {
        Terminal::render(self, changes)?;
        self.flush()?;
        Ok(())
    }

    fn size(&mut self) -> Result<(usize, usize)> {
        let size = self.get_screen_size()?;
        Ok((size.cols, size.rows))
    }
}

/// DumbTerm allows writing termwiz Changes to an arbitrary writer,
/// ignoring lack of ttyness and using a default terminal size.
pub(crate) struct DumbTerm<W: RenderTty + io::Write> {
    tty: W,
    renderer: TerminfoRenderer,
    separator: Option<u8>,
}

impl<W: RenderTty + io::Write> DumbTerm<W> {
    pub fn new(tty: W) -> Result<Self> {
        Ok(Self {
            tty,
            renderer: TerminfoRenderer::new(caps()?),
            separator: None,
        })
    }

    pub fn set_separator(&mut self, sep: u8) {
        self.separator = Some(sep);
    }
}

impl<W: RenderTty + io::Write> Term for DumbTerm<W> {
    fn render(&mut self, changes: &[Change]) -> Result<()> {
        self.renderer.render_to(changes, &mut self.tty)?;
        if let Some(sep) = self.separator {
            self.tty.write_all(&[sep])?;
            self.tty.flush()?;
        }
        Ok(())
    }

    fn size(&mut self) -> Result<(usize, usize)> {
        self.tty.get_size_in_cells()
    }
}

pub(crate) struct DumbTty {
    write: Box<dyn io::Write + Send + Sync>,
}

impl DumbTty {
    pub fn new(write: Box<dyn io::Write + Send + Sync>) -> Self {
        Self { write }
    }
}

impl RenderTty for DumbTty {
    fn get_size_in_cells(&mut self) -> Result<(usize, usize)> {
        Ok((DEFAULT_TERM_WIDTH, DEFAULT_TERM_HEIGHT))
    }
}

impl io::Write for DumbTty {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.write.flush()
    }
}

fn caps() -> Result<Capabilities> {
    let hints = termwiz::caps::ProbeHints::new_from_env().mouse_reporting(Some(false));
    termwiz::caps::Capabilities::new_with_hints(hints)
}

pub(crate) fn make_real_term() -> Result<Box<dyn Term + Send + Sync>> {
    // Don't use the real termwiz terminal yet because:
    //   1. On Windows, it disables automatic \n -> \r\n conversion.
    //   2. On Mac, we were detecting /dev/tty as usable, but ended up blocking when dropping the UnixTerminal object (when invoked via buck).
    //   3. Termwiz sets up a SIGWINCH handler which causes crash in Python crecord stuff.

    #[cfg(windows)]
    {
        let stderr = io::stderr();
        if stderr.is_tty() {
            let tty = windows_term::WindowsTty::new(Box::new(stderr));
            return Ok(Box::new(DumbTerm::new(tty)?));
        }
    }

    #[cfg(unix)]
    {
        let stderr = io::stderr();
        if stderr.is_tty() {
            let tty = unix_term::UnixTty::new(Box::new(stderr));
            return Ok(Box::new(DumbTerm::new(tty)?));
        }
    }

    termwiz::bail!("no suitable term output file");
}
