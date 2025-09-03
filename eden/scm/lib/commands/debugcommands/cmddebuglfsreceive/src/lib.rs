/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::io::Write;

use clidispatch::ReqCtx;
use clidispatch::abort;
use clidispatch::abort_if;
use cmdutil::ConfigSet;
use cmdutil::Error;
use cmdutil::Repo;
use cmdutil::Result;
use cmdutil::define_flags;
use revisionstore::LfsRemote;
use types::FetchContext;
use types::Sha256;

define_flags! {
    pub struct DebugLfsReceiveOpts {
        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<DebugLfsReceiveOpts>, _repo: Option<&Repo>) -> Result<u8> {
    let mut config = ConfigSet::wrap(ctx.config().clone());

    abort_if!(
        ctx.opts.args.len() < 2 || ctx.opts.args.len() > 3,
        "please specify 2 or 3 args"
    );

    let io = ctx.io().clone();

    let sha256 = Sha256::from_hex(ctx.opts.args[0].as_ref())?;
    let size: usize = ctx.opts.args[1].parse()?;

    if let Some(url) = ctx.opts.args.get(2) {
        config.set("lfs", "url", Some(url.clone()), &"debuglfsreceive".into());
    }

    let mut output = io.output();

    let lfs_remote = LfsRemote::from_config(&config)?;
    let mut error: Option<Error> = None;
    lfs_remote.batch_fetch(
        FetchContext::default(),
        &HashSet::from([(sha256, size)]),
        |_sha, data| Ok(output.write_all(data.as_ref())?),
        |_sha, err| error = Some(err),
    )?;

    if let Some(err) = error {
        abort!("error fetching LFS file: {err}");
    }

    Ok(0)
}

pub fn aliases() -> &'static str {
    "debuglfsreceive|debuglfsrecv"
}

pub fn doc() -> &'static str {
    "receive a single object from LFS server, write it to stdout"
}

pub fn synopsis() -> Option<&'static str> {
    Some("OID SIZE [URL]")
}

pub fn enable_cas() -> bool {
    false
}
