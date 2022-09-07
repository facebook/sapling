/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;

use anyhow::Result;
use cliparser::parser::ParseOutput;
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
}
