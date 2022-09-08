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
pub(crate) struct DumbTerm {
    tty: DumbTty,
    renderer: TerminfoRenderer,
}

impl DumbTerm {
    pub fn new(write: Box<dyn io::Write + Send + Sync>) -> Result<Self> {
        Ok(Self {
            tty: DumbTty { write },
            renderer: TerminfoRenderer::new(caps()?),
        })
    }
}

impl Term for DumbTerm {
    fn render(&mut self, changes: &[Change]) -> Result<()> {
        self.renderer.render_to(changes, &mut self.tty)
    }

    fn size(&mut self) -> Result<(usize, usize)> {
        self.tty.get_size_in_cells()
    }
}

struct DumbTty {
    write: Box<dyn io::Write + Send + Sync>,
}

impl RenderTty for DumbTty {
    fn get_size_in_cells(&mut self) -> Result<(usize, usize)> {
        Ok((80, 25))
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
