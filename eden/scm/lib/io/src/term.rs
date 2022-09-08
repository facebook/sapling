/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::OpenOptions;
use std::io;
#[cfg(unix)]
use std::os::unix::prelude::AsRawFd;

use termwiz::caps::Capabilities;
use termwiz::render::terminfo::TerminfoRenderer;
use termwiz::render::RenderTty;
use termwiz::surface::Change;
use termwiz::terminal::SystemTerminal;
use termwiz::terminal::Terminal;
use termwiz::Result;

pub(crate) const DEFAULT_TERM_WIDTH: usize = 80;
pub(crate) const DEFAULT_TERM_HEIGHT: usize = 25;

#[cfg(windows)]
use crate::IsTty;

#[cfg(windows)]
mod windows_term;

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
}

impl<W: RenderTty + io::Write> DumbTerm<W> {
    pub fn new(tty: W) -> Result<Self> {
        Ok(Self {
            tty,
            renderer: TerminfoRenderer::new(caps()?),
        })
    }
}

impl<W: RenderTty + io::Write> Term for DumbTerm<W> {
    fn render(&mut self, changes: &[Change]) -> Result<()> {
        self.renderer.render_to(changes, &mut self.tty)
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
    #[cfg(windows)]
    {
        // Don't use the real termwiz WindowsTerminal yet. See comment in WindowsTty.
        let stderr = io::stderr();
        if stderr.is_tty() {
            let tty = windows_term::WindowsTty::new(Box::new(stderr));
            return Ok(Box::new(DumbTerm::new(tty)?));
        }
    }

    #[cfg(unix)]
    {
        let caps = caps()?;

        // First try the tty. With this we can show progress even with
        // stdout and/or and stderr redirected.
        if let Ok(dev_tty) = OpenOptions::new().read(true).write(true).open("/dev/tty") {
            // Make sure our process is in the foreground process
            // group, otherwise we will block/fail writing to the tty.
            if is_foreground_process_group(dev_tty.as_raw_fd())? {
                if let Ok(term) = SystemTerminal::new_with(caps.clone(), &dev_tty, &dev_tty) {
                    return Ok(Box::new(term));
                }
            }
        }

        let stderr = io::stderr();
        if is_foreground_process_group(stderr.as_raw_fd())? {
            // Fall back to stderr (don't use stdout since that would
            // interfere with command output).
            return Ok(Box::new(SystemTerminal::new_with(
                caps,
                &io::stdin(),
                &stderr,
            )?));
        }
    }

    termwiz::bail!("no suitable term output file");
}

#[cfg(unix)]
/// Report whether the given fd is associated with a terminal and
/// our process is in the foreground process group.
fn is_foreground_process_group(fd: std::os::unix::prelude::RawFd) -> io::Result<bool> {
    let foreground_pg = unsafe { libc::tcgetpgrp(fd) };
    if foreground_pg < 0 {
        let err = io::Error::last_os_error();
        return match err.raw_os_error() {
            Some(libc::ENOTTY) => Ok(false),
            _ => Err(err),
        };
    }

    let my_pg = unsafe { libc::getpgid(0) };
    if my_pg < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(foreground_pg == my_pg)
}
