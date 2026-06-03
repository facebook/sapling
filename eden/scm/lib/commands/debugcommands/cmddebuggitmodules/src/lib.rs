/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use clidispatch::ReqCtx;
use cliparser::define_flags;
use cmdutil::Result;
use repo::Repo;
use workingcopy::workingcopy::WorkingCopy;

define_flags! {
    pub struct DebugGitModulesOpts {
        /// print in JSON string
        json: bool,
    }
}

pub fn run(ctx: ReqCtx<DebugGitModulesOpts>, repo: &Repo, wc: &WorkingCopy) -> Result<u8> {
    let gitmodules = if repo.requirements.contains("grepo") {
        wc.parse_grepo_submodules()?
    } else {
        wc.parse_submodule_config()?
    };

    if ctx.opts.json {
        let jstring = serde_json::to_string(&gitmodules)?;
        write!(ctx.io().output(), "{jstring}")?;
    } else {
        for m in gitmodules {
            write!(ctx.io().output(), "{m}")?;
        }
    }

    Ok(0)
}

pub fn aliases() -> &'static str {
    "debuggitmodules"
}

pub fn doc() -> &'static str {
    "List git submodules in the current working directory. \
Only applicable to git superproject and git repo tool."
}

pub fn synopsis() -> Option<&'static str> {
    None
}
