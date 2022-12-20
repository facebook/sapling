/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;

use anyhow::Result;
use cliparser::parser::ParseOutput;
use configmodel::convert::FromConfigValue;
use configmodel::Config;
use configmodel::ConfigExt;
use hgplain::is_plain;
use io::IsTty;
use io::IO;

use crate::global_flags::HgGlobalOpts;

/// CoreContext is a container for common facilities intended to be
/// passed into upper level library code.
pub struct CoreContext {
    pub io: IO,
    pub global_opts: HgGlobalOpts,
}

/// RequestContext is a container object to organize CLI facilities.
pub struct RequestContext<O>
where
    O: TryFrom<ParseOutput, Error = anyhow::Error>,
{
    pub core: CoreContext,
    pub opts: O,
}

impl<O> RequestContext<O>
where
    O: TryFrom<ParseOutput, Error = anyhow::Error>,
{
    pub(crate) fn new(p: ParseOutput, io: IO) -> Result<Self> {
        Ok(Self {
            core: CoreContext {
                io,
                global_opts: p.clone().try_into()?,
            },
            opts: p.try_into()?,
        })
    }

    pub fn io(&self) -> &IO {
        &self.core.io
    }

    pub fn global_opts(&self) -> &HgGlobalOpts {
        &self.core.global_opts
    }

    pub fn maybe_start_pager(&self, config: &dyn Config) -> Result<()> {
        let (enable_pager, reason) =
            if bool::try_from_str(&self.core.global_opts.pager).unwrap_or(false) {
                (true, "--pager")
            } else if is_plain(Some("pager")) {
                (false, "plain")
            } else if self.core.global_opts.pager != "auto" {
                (false, "--pager")
            } else if !self.core.io.output().is_tty() {
                (false, "not tty")
            } else if !config.get_or("ui", "paginate", || true)? {
                (false, "ui.paginate")
            } else {
                (true, "auto")
            };

        tracing::debug!(enable_pager, reason, "maybe starting pager");

        if enable_pager {
            self.core.io.start_pager(config)?;
        }

        Ok(())
    }
}
