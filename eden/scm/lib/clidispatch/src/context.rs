/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;
use std::sync::Arc;

use anyhow::Result;
use cliparser::parser::ParseOutput;
use configmodel::convert::FromConfigValue;
use configmodel::Config;
use configmodel::ConfigExt;
use context::CoreContext;
use hgplain::is_plain;
use io::IsTty;
use io::IO;
use termlogger::TermLogger;

use crate::global_flags::HgGlobalOpts;

/// RequestContext is a container object to organize CLI facilities.
pub struct RequestContext<O>
where
    O: TryFrom<ParseOutput, Error = anyhow::Error>,
{
    pub core: CoreContext,
    pub opts: O,
    pub global_opts: HgGlobalOpts,
}

impl<O> RequestContext<O>
where
    O: TryFrom<ParseOutput, Error = anyhow::Error>,
{
    pub(crate) fn new(config: Arc<dyn Config>, p: ParseOutput, io: IO) -> Result<Self> {
        let global_opts: HgGlobalOpts = p.clone().try_into()?;
        Ok(Self {
            core: CoreContext::new(config, io, p.raw_args.clone()),
            opts: p.try_into()?,
            global_opts,
        })
    }

    pub fn io(&self) -> &IO {
        &self.core.io
    }

    pub fn global_opts(&self) -> &HgGlobalOpts {
        &self.global_opts
    }

    pub fn maybe_start_pager(&self, config: &dyn Config) -> Result<()> {
        let (enable_pager, reason) = if bool::try_from_str(&self.global_opts.pager).unwrap_or(false)
        {
            (true, "--pager")
        } else if is_plain(Some("pager")) {
            (false, "plain")
        } else if self.global_opts.pager != "auto" {
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

    pub fn logger(&self) -> TermLogger {
        self.core.logger.clone()
    }

    pub fn config(&self) -> &Arc<dyn Config> {
        &self.core.config
    }
}
