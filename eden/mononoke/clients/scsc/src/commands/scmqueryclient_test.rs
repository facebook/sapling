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
//! Method coverage grows over the diff stack — each diff that ports a new
//! ScmQueryService method to SCS-direct also adds the corresponding
//! `Method::<Method>` variant + args struct + match arm here, and a `.t`
//! test that invokes `scsc scmqueryclient-test <method>` against a tiny
//! Mononoke fixture repo.

use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use clap::Args;
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

#[derive(Subcommand)]
enum Method {
    /// cat_v2: read a file (or directory) at a rev.
    CatV2(CatV2Args),
    /// is_ancestor: check whether one commit is an ancestor of another.
    IsAncestor(IsAncestorArgs),
    /// get_generation: fetch the DAG generation number of a commit.
    GetGeneration(GetGenerationArgs),
}

#[derive(Args)]
struct CatV2Args {
    #[arg(long)]
    repo: String,
    #[arg(long, default_value = "hg")]
    scm_type: String,
    #[arg(long)]
    rev: String,
    #[arg(long)]
    path: String,
}

#[derive(Args)]
struct IsAncestorArgs {
    #[arg(long)]
    repo: String,
    #[arg(long, default_value = "hg")]
    scm_type: String,
    #[arg(long)]
    maybe_ancestor: String,
    #[arg(long)]
    maybe_descendant: String,
}

#[derive(Args)]
struct GetGenerationArgs {
    #[arg(long)]
    repo: String,
    #[arg(long, default_value = "hg")]
    scm_type: String,
    #[arg(long)]
    rev: String,
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let wrapper = build_wrapper(&app, &args.client_id).await?;
    match args.method {
        Some(Method::CatV2(a)) => {
            let params = scmquery_types::ScmCatParams {
                repo: a.repo,
                scm_type: a.scm_type,
                rev: a.rev,
                path: a.path,
                ..Default::default()
            };
            let bytes = wrapper.cat_v2(&params).await?;
            std::io::stdout().write_all(&bytes)?;
        }
        Some(Method::IsAncestor(a)) => {
            let params = scmquery_types::ScmIsAncestorParams {
                repo: a.repo,
                scm_type: a.scm_type,
                maybe_ancestor: a.maybe_ancestor,
                maybe_descendant: a.maybe_descendant,
                ..Default::default()
            };
            let result = wrapper.is_ancestor(&params).await?;
            println!("{result}");
        }
        Some(Method::GetGeneration(a)) => {
            let params = scmquery_types::ScmGetGenerationParams {
                repo: a.repo,
                scm_type: a.scm_type,
                rev: a.rev,
                ..Default::default()
            };
            let result = wrapper.get_generation(&params).await?;
            println!("{}", result.generation);
        }
        None => bail!(
            "no method specified; pass one of the per-method subcommands. \
             See `scsc scmqueryclient-test --help` for the list."
        ),
    }
    Ok(())
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
