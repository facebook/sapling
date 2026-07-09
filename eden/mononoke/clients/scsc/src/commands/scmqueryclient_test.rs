/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Hidden test subcommand for exercising the `scmqueryclient-rust` library
//! end-to-end against a real Source Control Service.
//!
//! Gated by the `SCSC_SCMQUERY_TEST_ENABLED` env var so the subcommand
//! doesn't appear in normal `scsc --help` output and isn't accessible in
//! production CLI usage.
//!
//! This file ships as scaffolding only — each subsequent diff in the
//! `scmquery_client: SCS-direct <method>` stack adds:
//! 1. a `Method::<Method>` variant on the `Method` enum,
//! 2. an `Args` struct for that variant,
//! 3. a match arm in `run()` that calls the corresponding wrapper method,
//! 4. a `.t` test that invokes `scsc scmqueryclient-test <method>` against
//!    a tiny Mononoke fixture repo.

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use clap::Parser;
use clap::Subcommand;
use identity::IdentitySet;
use scmqueryclient_rust::SRClientConfig;
use scmqueryclient_rust::ScmQuery;
use scmqueryclient_rust::ScmQueryClient;
use source_control_clients::SourceControlService;
use source_control_thriftclients::make_SourceControlService_thriftclient;

use crate::ScscApp;

#[derive(Parser)]
pub(super) struct CommandArgs {
    #[arg(long, default_value = "scsc-scmqueryclient-test")]
    client_id: String,

    #[command(subcommand)]
    method: Option<Method>,
}

/// Per-method variants are added by the diffs that port each method to
/// SCS-direct. This empty form is the scaffolding-only state.
#[derive(Subcommand)]
enum Method {}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let _wrapper = build_wrapper(&app, &args.client_id).await?;
    match args.method {
        Some(m) => match m {},
        None => bail!(
            "no method specified; the scmqueryclient-test subcommand is built up \
             incrementally per port - see the diff stack starting from \
             D106079254 for the per-method ports"
        ),
    }
}

async fn build_wrapper(app: &ScscApp, client_id: &str) -> Result<Box<dyn ScmQueryClient>> {
    let sr_config = SRClientConfig::default();

    let host_port = app.scs_host().ok_or_else(|| {
        anyhow::anyhow!(
            "scmqueryclient-test requires -H/--host, set by the library.sh scsc wrapper"
        )
    })?;

    let identity = std::env::var("MONONOKE_INTEGRATION_TEST_EXPECTED_THRIFT_SERVER_IDENTITY")
        .context("MONONOKE_INTEGRATION_TEST_EXPECTED_THRIFT_SERVER_IDENTITY must be set by the library.sh scsc wrapper")?;
    let identity_parsed = identity
        .parse()
        .with_context(|| format!("invalid expected thrift server identity {identity:?}"))?;
    let expected_identities = IdentitySet::from_iter(std::iter::once(identity_parsed));

    let addr = tokio::net::lookup_host(host_port)
        .await
        .map_err(|e| anyhow::anyhow!("invalid scs host {host_port}: {e}"))?
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid scs host {host_port}: resolved to no addresses"))?;

    let scs_client: Arc<dyn SourceControlService + Send + Sync> = make_SourceControlService_thriftclient!(
        app.fb,
        from_sock_addr = addr,
        with_conn_timeout = 5_000u32,
        with_recv_timeout = 30_000u32,
        with_secure = true,
        with_expected_identities = expected_identities,
    )?;

    Ok(Box::new(
        ScmQuery::new_with_config(app.fb, Some(client_id), sr_config)
            .get_raw_client_v2_with_scs_client(scs_client),
    ))
}
