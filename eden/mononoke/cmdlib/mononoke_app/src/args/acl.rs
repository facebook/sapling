/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use clap::Args;

/// Command line arguments for controlling Acls
#[derive(Args, Debug)]
pub struct AclArgs {
    /// Load ACLs from a JSON-formatted file.
    #[clap(long, value_parser)]
    pub acl_file: Option<PathBuf>,
}
