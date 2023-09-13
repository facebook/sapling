/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clidispatch::ReqCtx;
#[cfg(feature = "fb")]
use configloader::hg::generate_dynamicconfig;
#[cfg(feature = "fb")]
use configmodel::Config;
#[cfg(feature = "fb")]
use configmodel::ConfigExt;

use super::define_flags;
use super::Repo;
use super::Result;

define_flags! {
    pub struct DebugDynamicConfigOpts {
        /// Host name to fetch a canary config from.
        canary: Option<String>,
    }
}

pub fn run(ctx: ReqCtx<DebugDynamicConfigOpts>, repo: &mut Repo) -> Result<u8> {
    #[cfg(feature = "fb")]
    {
        use configloader::fb::FbConfigMode;
        let username = repo
            .config()
            .get("ui", "username")
            .map(|u| u.to_string())
            .unwrap_or_else(|| "".to_string());

        let mode = FbConfigMode::default();

        generate_dynamicconfig(
            mode,
            Some(repo.shared_dot_hg_path()),
            repo.repo_name(),
            ctx.opts.canary,
            username,
            repo.config().get_opt("auth_proxy", "unix_socket_path")?,
        )?;
    }
    #[cfg(not(feature = "fb"))]
    let _ = (ctx, repo);

    Ok(0)
}

pub fn aliases() -> &'static str {
    "debugdynamicconfig"
}

pub fn doc() -> &'static str {
    "generate the dynamic configuration"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
