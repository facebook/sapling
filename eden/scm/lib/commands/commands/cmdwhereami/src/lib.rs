/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use clidispatch::ReqCtx;
use cmdutil::NoOpts;
use cmdutil::Result;
use repo::repo::Repo;
use types::hgid::NULL_ID;

pub fn run(ctx: ReqCtx<NoOpts>, repo: &Repo) -> Result<u8> {
    let parents = workingcopy::fast_path_wdir_parents(repo.path(), repo.ident())?;
    let p1 = parents.p1().copied().unwrap_or(NULL_ID);

    let mut stdout = ctx.io().output();
    write!(stdout, "{}\n", p1.to_hex())?;

    if let Some(p2) = parents.p2() {
        write!(stdout, "{}\n", p2.to_hex())?;
    }

    Ok(0)
}

pub fn aliases() -> &'static str {
    "whereami"
}

pub fn doc() -> &'static str {
    r#"output the working copy's parent hashes

If there are no parents, an all zeros hash is emitted.
If there are two parents, both will be emitted, newline separated.
"#
}

pub fn synopsis() -> Option<&'static str> {
    None
}

pub fn enable_cas() -> bool {
    false
}
