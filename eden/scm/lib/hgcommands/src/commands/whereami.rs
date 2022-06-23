/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::File;
use std::io::Write;

use anyhow::Context;
use treestate::serialization::Serializable;
use types::HgId;

use super::define_flags;
use super::Repo;
use super::Result;
use super::IO;

define_flags! {
    pub struct WhereamiOpts {
    }
}

pub fn run(_opts: WhereamiOpts, io: &IO, repo: &mut Repo) -> Result<u8> {
    let mut stdout = io.output();

    let dirstate_path = repo.dot_hg_path().join("dirstate");
    let mut dirstate_file = match File::open(&dirstate_path) {
        Ok(f) => f,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            // Show zeros to indicate lack of parent.
            write!(stdout, "{}\n", HgId::null_id().to_hex())?;
            return Ok(0);
        }
        Err(err) => {
            return Err(err).with_context(|| {
                format!("error opening dirstate file {}", dirstate_path.display())
            });
        }
    };

    let dirstate = treestate::dirstate::Dirstate::deserialize(&mut dirstate_file)?;

    write!(stdout, "{}\n", dirstate.p0.to_hex())?;
    if !dirstate.p1.is_null() {
        write!(stdout, "{}\n", dirstate.p1.to_hex())?;
    }

    Ok(0)
}

pub fn name() -> &'static str {
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
