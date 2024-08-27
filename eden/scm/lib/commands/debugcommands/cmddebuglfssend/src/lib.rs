/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::io::Read;
use std::io::Write;

use clidispatch::abort;
use clidispatch::abort_if;
use clidispatch::ReqCtx;
use cmdutil::define_flags;
use cmdutil::ConfigSet;
use cmdutil::Error;
use cmdutil::Result;
use minibytes::Bytes;
use revisionstore::LfsRemote;
use sha2::Digest;
use types::Sha256;

define_flags! {
    pub struct DebugLfsSendOpts {
        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<DebugLfsSendOpts>) -> Result<u8> {
    let mut config = ConfigSet::wrap(ctx.config().clone());
    abort_if!(ctx.opts.args.len() > 1, "too many args");

    let io = ctx.io().clone();

    if let Some(url) = ctx.opts.args.into_iter().next() {
        config.set("lfs", "url", Some(url), &"debuglfssend".into());
    }

    let mut input = io.input();
    let mut content = Vec::<u8>::new();
    input.read_to_end(&mut content)?;
    let sha256 = Sha256::from_slice(sha2::Sha256::digest(&content).as_ref())?;
    let size = content.len();

    let content: Bytes = content.into();

    let lfs_remote = LfsRemote::from_config(&config)?;
    let mut error: Option<Error> = None;
    lfs_remote.batch_upload(
        &HashSet::from([(sha256, size)]),
        move |_sha, _size| Ok(Some(content.clone())),
        |_sha, err| error = Some(err),
    )?;

    if let Some(err) = error {
        abort!("error uploading LFS file: {err}");
    }

    let mut output = io.output();
    writeln!(output, "{sha256} {size}")?;

    Ok(0)
}

pub fn aliases() -> &'static str {
    "debuglfssend"
}

pub fn doc() -> &'static str {
    "read from stdin, send it as a single file to LFS server

    Print oid and size."
}

pub fn synopsis() -> Option<&'static str> {
    Some("[URL]")
}
