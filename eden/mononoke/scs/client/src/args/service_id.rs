/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Arguments for Service Identities

#[derive(clap::Args)]
pub(crate) struct ServiceIdArgs {
    #[clap(long)]
    /// Service identity to perform write operation as
    pub(crate) service_id: Option<String>,
}
