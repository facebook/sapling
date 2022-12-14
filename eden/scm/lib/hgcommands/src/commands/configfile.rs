/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Context;
use clidispatch::abort;
use clidispatch::abort_if;
use clidispatch::OptionalRepo;
use clidispatch::ReqCtx;
use cliparser::define_flags;
use configloader::hg::all_existing_system_paths;
use configloader::hg::all_existing_user_paths;

use super::Result;

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

pub fn run(ctx: ReqCtx<DebugConfigLocationOpts>, repo: &mut OptionalRepo) -> Result<u8> {
    let optcnt = (ctx.opts.user as i32) + (ctx.opts.local as i32) + (ctx.opts.system as i32);

    abort_if!(
        optcnt > 1,
        "must select at most one of --user, --local, or --system"
    );

    let show_all = optcnt == 0;

    if show_all || ctx.opts.user {
        let id = identity::default();
        let path = all_existing_user_paths(&id)
            .chain(id.user_config_paths().into_iter())
            .next()
            .with_context(|| "unable to determine user config location")?;
        if show_all {
            write!(ctx.io().output(), "User config path: ")?;
        }
        write!(ctx.io().output(), "{}\n", path.display())?;
    }

    if show_all || ctx.opts.local {
        if let OptionalRepo::Some(repo) = repo {
            if show_all {
                write!(ctx.io().output(), "Repo config path: ")?;
            }
            write!(ctx.io().output(), "{}\n", repo.config_path().display())?;
        } else if !show_all {
            abort!("--local must be used inside a repo");
        }
    }

    if show_all || ctx.opts.system {
        let id = identity::default();
        let path = all_existing_system_paths(&id)
            .chain(id.system_config_path().into_iter())
            .next()
            .with_context(|| "unable to determine system config location")?;
        if show_all {
            write!(ctx.io().output(), "System config path: ")?;
        }
        write!(ctx.io().output(), "{}\n", path.display())?;
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
