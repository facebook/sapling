/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Args;

/// Command line arguments for TLS parameters
#[derive(Args, Debug, Clone)]
#[group(requires_all = ["tls_certificate", "tls_private_key", "tls_ca"])]
pub struct TLSArgs {
    /// TLS Certificate for auth
    #[clap(long)]
    #[arg(required = false)]
    pub tls_certificate: String,
    /// TLS private key
    #[clap(long)]
    #[arg(required = false)]
    pub tls_private_key: String,
    /// TLS CA
    #[clap(long)]
    #[arg(required = false)]
    pub tls_ca: String,
    /// TLS Ticket Seeds
    #[clap(long)]
    pub tls_ticket_seeds: Option<String>,
    #[clap(long)]
    pub disable_mtls: bool,
}
