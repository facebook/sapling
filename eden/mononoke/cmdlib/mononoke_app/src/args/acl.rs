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

    /// Use the `AccessChecker` library (`access/lib` via ligen) for ACL checks
    /// instead of the default `aclchecker`-backed provider. Off by default.
    #[clap(long)]
    pub access_checker_enabled: bool,

    /// Verifier identity (`<type>:<data>`) for the AccessChecker library. Used
    /// when `--access-checker-enabled` is set to construct the `AccessChecker`
    /// instance.
    #[clap(long, default_value = "SERVICE_IDENTITY:scm_service_identity")]
    pub access_checker_verifier: String,
}
