/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use fbinit::FacebookInit;

/// Command line arguments for controlling GFlags
#[derive(Args, Debug)]
pub struct GFlagsArgs {
    /// Mononoke doesn't use gflags but some of its deps do. This arg allows to override them
    /// any gflags provided as key=value pairs
    #[clap(long)]
    pub gflag: Vec<String>,
}

impl GFlagsArgs {
    /// Propagate gflags to the gflags library
    pub(crate) fn propagate(&self, fb: FacebookInit) -> Result<()> {
        #[cfg(fbcode_build)]
        for flag in &self.gflag {
            let (key, value) = flag
                .rsplit_once('=')
                .ok_or_else(|| anyhow::anyhow!("Invalid flag value: {}", flag))?;
            gflags::set_raw_gflag_value(fb, key, value)?;
        }
        #[cfg(not(fbcode_build))]
        {
            let _ = fb;
            if self.gflag.len() > 0 {
                anyhow::bail!("GFlagsArgs is only supported in fbcode builds")
            }
        }
        Ok(())
    }
}
