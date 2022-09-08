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

pub(crate) fn make_real_term() -> Result<Box<dyn Term + Send + Sync>> {
    let caps = caps()?;

    #[cfg(windows)]
    return Ok(Box::new(SystemTerminal::new(caps.clone()).or_else(
        |_err| {
            // Fall back to stderr since that won't interfere with command output.
            SystemTerminal::new_with(caps.clone(), io::stdin(), io::stderr())
        },
    )?));

    #[cfg(unix)]
    {
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
                caps.clone(),
                &io::stdin(),
                &stderr,
            )?));
        }

        termwiz::bail!("no suitable term output file");
    }
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
