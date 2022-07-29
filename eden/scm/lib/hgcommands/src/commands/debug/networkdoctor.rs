/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use clidispatch::OptionalRepo;
use configparser::config::Options;

use super::define_flags;
use super::Result;
use super::IO;

define_flags! {
    pub struct DebugNetworkDoctorOps {
    }
}

pub fn run(_opts: DebugNetworkDoctorOps, io: &IO, repo: &mut OptionalRepo) -> Result<u8> {
    // Set a default repo so we can build valid edenapi URLs outside a repo.
    if let OptionalRepo::None(ref mut config) = repo {
        config.set(
            "remotefilelog",
            "reponame",
            Some("fbsource"),
            &Options::new().source("networkdoctor.rs"),
        );
    }

    let mut stdout = io.output();
    match network_doctor::Doctor::new().diagnose(repo.config()) {
        Ok(()) => write!(stdout, "No network problems detected.\n")?,
        Err(d) => write!(stdout, "{}\n\n{}\n", d.treatment(repo.config()), d)?,
    };
    Ok(0)
}

pub fn name() -> &'static str {
    "debugnetworkdoctor"
}

pub fn doc() -> &'static str {
    "run the (Rust) network doctor"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
