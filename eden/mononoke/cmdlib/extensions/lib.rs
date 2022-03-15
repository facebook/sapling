/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// Trait allows to configure defaults for the clap::Args, which can be
/// different for different Mononoke binaries.
pub trait ArgDefaults {
    // TODO: We may want to add the Args type defaults are set for and
    // extend the trait with more methods.
    // type Args: clap::Args;

    /// Obtain default values for these arguments.
    fn arg_defaults(&self) -> Vec<(&'static str, String)>;
}
