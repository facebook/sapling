/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use io::IO;
use lazystr::LazyStr;

/// TermLogger mixes the IO object with knowledge of output verbosity.
pub struct TermLogger {
    io: IO,
    quiet: bool,
    verbose: bool,
}

impl TermLogger {
    pub fn new(io: &IO) -> Self {
        TermLogger {
            io: io.clone(),
            quiet: false,
            verbose: false,
        }
    }

    pub fn null() -> Self {
        TermLogger {
            io: IO::null(),
            quiet: false,
            verbose: false,
        }
    }

    pub fn with_quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Write to stdout if not --quiet.
    pub fn info(&self, msg: impl LazyStr) {
        if !self.quiet {
            Self::write(self.io.output(), msg.to_str())
        }
    }

    /// Write to stderr.
    pub fn warn(&self, msg: impl AsRef<str>) {
        Self::write(self.io.error(), msg)
    }

    /// Write to stdout if --verbose.
    pub fn verbose(&self, msg: impl LazyStr) {
        if self.verbose {
            Self::write(self.io.output(), msg.to_str())
        }
    }

    /// Short client program name.
    pub fn cli_name(&self) -> &'static str {
        identity::cli_name()
    }

    pub fn flush(&self) {
        let _ = self.io.flush();
    }

    pub fn io(&self) -> &IO {
        &self.io
    }

    fn write(mut w: impl Write, msg: impl AsRef<str>) {
        let msg = identity::default().punch(msg.as_ref());

        if let Err(err) = || -> std::io::Result<()> {
            w.write_all(msg.as_bytes())?;
            if !msg.ends_with('\n') {
                w.write_all(b"\n")?;
            }
            Ok(())
        }() {
            tracing::warn!(?msg, ?err, "error writing command output");
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_quiet() {
        let io = IO::new("".as_bytes(), Vec::new(), Some(Vec::new()));
        let logger = TermLogger::new(&io).with_quiet(true);
        logger.info("hello");
        logger.warn("error");
        assert_eq!(get_stdout(&io), "");
        assert_eq!(get_stderr(&io), "error\n");
    }

    #[test]
    fn test_default() {
        let io = IO::new("".as_bytes(), Vec::new(), Some(Vec::new()));
        let logger = TermLogger::new(&io);
        logger.info("status");
        logger.verbose("verbose");
        logger.warn("warn");
        assert_eq!(get_stdout(&io), "status\n");
        assert_eq!(get_stderr(&io), "warn\n");
    }

    #[test]
    fn test_verbose() {
        let io = IO::new("".as_bytes(), Vec::new(), Some(Vec::new()));
        let logger = TermLogger::new(&io);
        logger.verbose(|| -> String {
            panic!("don't call me!");
        });
        assert_eq!(get_stdout(&io), "");

        let logger = logger.with_verbose(true);
        logger.verbose(|| "okay".to_string());
        assert_eq!(get_stdout(&io), "okay\n");
    }

    fn get_stdout(io: &IO) -> String {
        let stdout = io.with_output(|o| o.as_any().downcast_ref::<Vec<u8>>().unwrap().clone());
        String::from_utf8(stdout).unwrap()
    }

    fn get_stderr(io: &IO) -> String {
        let stderr = io.with_error(|o| {
            o.unwrap()
                .as_any()
                .downcast_ref::<Vec<u8>>()
                .unwrap()
                .clone()
        });
        String::from_utf8(stderr).unwrap()
    }
}
