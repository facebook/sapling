/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use configmodel::Config;
use io::IO;
use termlogger::TermLogger;

/// Context is a container for common facilities intended to be
/// passed into upper level library code.
pub struct CoreContext {
    pub config: Arc<dyn Config>,
    pub io: IO,
    pub logger: TermLogger,
    pub raw_args: Vec<String>,
}
