/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;
use fbinit::FacebookInit;
use scribe_ext::Scribe;

/// Command line argument that affect scribe logging
#[derive(Args, Debug)]
pub struct ScribeLoggingArgs {
    /// Filesystem directory where to log all scribe writes
    #[clap(long)]
    pub scribe_logging_directory: Option<String>,
}

impl ScribeLoggingArgs {
    pub fn get_scribe(&self, fb: FacebookInit) -> Result<Scribe> {
        match &self.scribe_logging_directory {
            Some(dir) => Ok(Scribe::new_to_file(PathBuf::from(dir))),
            None => Ok(Scribe::new(fb)),
        }
    }
}
