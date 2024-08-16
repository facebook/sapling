/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use clidispatch::abort;
use clidispatch::abort_if;
use clidispatch::ReqCtx;
use cliparser::define_flags;
use cmdutil::Repo;
use cmdutil::Result;

define_flags! {
    pub struct DebugConfigLocationOpts {
        /// show the path of the user's config
        #[short('u')]
        user: bool,

        /// show the path of the current repo (if inside a repo)
        #[short('l')]
        local: bool,

        /// show the path of the system config file
        #[short('s')]
        system: bool,
    }
}

pub fn run(ctx: ReqCtx<DebugConfigLocationOpts>, repo: Option<&Repo>) -> Result<u8> {
    let optcnt = (ctx.opts.user as i32) + (ctx.opts.local as i32) + (ctx.opts.system as i32);

    abort_if!(
        optcnt > 1,
        "must select at most one of --user, --local, or --system"
    );

    let show_all = optcnt == 0;
    let mut out = ctx.io().output();

    if show_all || ctx.opts.user {
        let id = identity::default();
        if let Some(path) = id.user_config_path() {
            if show_all {
                write!(out, "User config path: ")?;
            }
            write!(out, "{}\n", path.display(),)?;
        }
    }

    if show_all || ctx.opts.local {
        if let Some(repo) = repo {
            if show_all {
                write!(out, "Repo config path: ")?;
            }
            write!(out, "{}\n", repo.config_path().display())?;
        } else if !show_all {
            abort!("--local must be used inside a repo");
        }
    }

    if show_all || ctx.opts.system {
        let id = identity::default();
        let paths = id.system_config_paths();
        if let Some(path) = paths.first() {
            if show_all {
                write!(out, "System config path: ")?;
            }
            write!(out, "{}\n", path.display())?;
        }
    }

    Ok(0)
}

pub fn aliases() -> &'static str {
    "configfile"
}

pub fn doc() -> &'static str {
    "shows the location of the selected config file"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
