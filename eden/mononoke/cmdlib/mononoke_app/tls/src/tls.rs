/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Args;

/// Command line arguments for TLS parameters
#[derive(Args, Debug)]
pub struct TLSArgs {
    /// TLS Certificate for auth
    #[clap(long)]
    pub tls_certificate: Option<String>,
    /// TLS private key
    #[clap(long)]
    pub tls_private_key: Option<String>,
    /// TLS CA
    #[clap(long)]
    pub tls_ca: Option<String>,
    /// TLS Ticket Seeds
    #[clap(long)]
    pub tls_ticket_seeds: Option<String>,
}
