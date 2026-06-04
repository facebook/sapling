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
    /// Mutually exclusive with `--access-checker-shadow-enabled`.
    #[clap(long, conflicts_with = "access_checker_shadow_enabled")]
    pub access_checker_enabled: bool,

    /// Run `AccessChecker` alongside the default provider in shadow mode: both
    /// providers handle every check, the legacy result is returned to callers,
    /// and divergences are logged via `tracing::warn!` with the
    /// `[acl_checker_shadow]` prefix. Useful for validating the AccessChecker
    /// path against the production provider before flipping
    /// `--access-checker-enabled`. Off by default. Mutually exclusive with
    /// `--access-checker-enabled`.
    #[clap(long)]
    pub access_checker_shadow_enabled: bool,

    /// Verifier identity (`<type>:<data>`) for the AccessChecker library. Used
    /// when `--access-checker-enabled` or `--access-checker-shadow-enabled` is
    /// set to construct the `AccessChecker` instance.
    #[clap(long, default_value = "SERVICE_IDENTITY:scm_service_identity")]
    pub access_checker_verifier: String,

    /// Sampling rate for non-divergent shadow-mode check samples. For every
    /// `N` checks per checker, one is logged to the
    /// `mononoke_shadow_perm_checker` Scuba dataset as a baseline regardless
    /// of whether primary and shadow agreed. Divergences are always logged
    /// independently of this rate. Set to `0` to disable baseline sampling.
    /// Only meaningful with `--access-checker-shadow-enabled`.
    #[clap(long, default_value = "10000")]
    pub access_checker_shadow_sample_rate: u64,
}
