/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use super::define_flags;
use super::Repo;
use super::Result;
use super::IO;

define_flags! {
    pub struct DebugNetworkDoctorOps {
    }
}

pub fn run(_opts: DebugNetworkDoctorOps, io: &IO, repo: Repo) -> Result<u8> {
    write!(
        io.output(),
        "{:?}\n",
        network_doctor::Doctor::new().diagnose(repo.config())
    )?;
    Ok(0)
}

pub fn name() -> &'static str {
    "debugnetworkdoctor"
}

pub fn doc() -> &'static str {
    "run the (Rust) network doctor"
}
