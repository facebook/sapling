/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use io::IO;
pub use termlogger::TermLogger;

use crate::global_flags::HgGlobalOpts;

pub fn new_logger(io: &IO, opts: &HgGlobalOpts) -> TermLogger {
    TermLogger::new(io)
        .with_quiet(opts.quiet)
        .with_verbose(opts.verbose)
        .with_debug(opts.debug)
}
