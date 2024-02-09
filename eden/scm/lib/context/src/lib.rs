/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use configmodel::Config;
use configmodel::ConfigExt;
use io::IO;
use termlogger::TermLogger;

/// Context is a container for common facilities intended to be
/// passed into upper level library code.
#[derive(Clone)]
pub struct CoreContext {
    pub config: Arc<dyn Config>,
    pub io: IO,
    pub logger: TermLogger,
    pub raw_args: Vec<String>,
}

impl CoreContext {
    pub fn new(config: Arc<dyn Config>, io: IO, raw_args: Vec<String>) -> Self {
        let logger = TermLogger::new(&io)
            .with_quiet(config.get_or_default("ui", "quiet").unwrap_or_default())
            .with_verbose(config.get_or_default("ui", "verbose").unwrap_or_default());
        Self {
            config,
            io,
            logger,
            raw_args,
        }
    }

    pub fn with_null_logger(&self) -> Self {
        let mut ctx = self.clone();
        ctx.logger = TermLogger::null();
        ctx
    }
}
