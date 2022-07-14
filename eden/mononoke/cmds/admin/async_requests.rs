/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::value_t;
use context::CoreContext;
use context::SessionContainer;
use fbinit::FacebookInit;
use maplit::hashmap;
use megarepo_api::MegarepoApi;
use mononoke_types::ChangesetId;
use mononoke_types::Timestamp;
use prettytable::cell;
use prettytable::format;
use prettytable::row;
use prettytable::Table;
use slog::Logger;

use crate::error::SubcommandError;

use async_requests::types::MegarepoAsynchronousRequestResult;
use async_requests::types::RequestStatus;
use async_requests::types::RowId;
use async_requests::types::ThriftMegarepoAsynchronousRequestParams;
use megarepo_error::MegarepoError;
use mononoke_api::Mononoke;
use mononoke_api::MononokeApiEnvironment;
use mononoke_api::WarmBookmarksCacheDerivedData;
use repo_factory::RepoFactory;
use scuba_ext::MononokeScubaSampleBuilder;
use source_control::MegarepoAddBranchingTargetResult;
use source_control::MegarepoAddTargetResult;
use source_control::MegarepoChangeTargetConfigResult;
use source_control::MegarepoRemergeSourceResult;
use source_control::MegarepoSyncChangesetResult;

pub const ASYNC_REQUESTS: &str = "async-requests";
const LIST_CMD: &str = "list";
pub const LOOKBACK_SECS: &str = "lookback";

const SHOW_CMD: &str = "show";
const REQUEUE_CMD: &str = "requeue";
const ABORT_CMD: &str = "abort";
pub const REQUEST_ID_ARG: &str = "request-id";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    let list = SubCommand::with_name(LIST_CMD)
         .about(
             "lists asynchronous requests (by default the ones active now or updated within last 5 mins)",
         ).arg(Arg::with_name(LOOKBACK_SECS)
            .long(LOOKBACK_SECS)
            .value_name("N")
            .help("limit the results to the requests updated in the last N seconds")
            .default_value("3600")
            .takes_value(true)
        );

    let show = SubCommand::with_name(SHOW_CMD)
        .about("shows request details")
        .arg(
            Arg::with_name(REQUEST_ID_ARG)
                .value_name("ID")
                .help("id of the request")
                .takes_value(true),
        );

    let requeue = SubCommand::with_name(REQUEUE_CMD)
        .about("changes the request status to new so it's picked up by workers again")
        .arg(
            Arg::with_name(REQUEST_ID_ARG)
                .value_name("ID")
                .help("id of the request")
                .takes_value(true),
        );

    let abort = SubCommand::with_name(ABORT_CMD)
        .about(
            "Changes the request status to ready and put error as result. \
               (this won't stop any currently running workers immediately)",
        )
        .arg(
            Arg::with_name(REQUEST_ID_ARG)
                .value_name("ID")
                .help("id of the request")
                .takes_value(true),
        );

    SubCommand::with_name(ASYNC_REQUESTS)
        .about("view and manage the SCS async requests (used by megarepo)")
        .subcommand(list)
        .subcommand(show)
        .subcommand(abort)
        .subcommand(requeue)
}

pub async fn subcommand_async_requests<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'a>,
    sub_m: &'a ArgMatches<'a>,
) -> Result<(), SubcommandError> {
    let config_store = matches.config_store();
    let (repo_name, repo_config) = args::get_config(config_store, matches)?;
    let common_config = args::load_common_config(config_store, matches)?;
    let repo_configs = args::RepoConfigs {
        repos: hashmap! {
            repo_name => repo_config
        },
        common: common_config,
    };
    let repo_factory = RepoFactory::new(matches.environment().clone(), &repo_configs.common);
    let env = MononokeApiEnvironment {
        repo_factory: repo_factory.clone(),
        warm_bookmarks_cache_derived_data: WarmBookmarksCacheDerivedData::None,
        skiplist_enabled: false,
        warm_bookmarks_cache_enabled: false,
        warm_bookmarks_cache_scuba_sample_builder: MononokeScubaSampleBuilder::with_discard(),
    };
    let mononoke = Arc::new(
        Mononoke::new(&env, repo_configs.clone())
            .await
            .context("Failed to initialize Mononoke API")?,
    );
    let megarepo = MegarepoApi::new(matches.environment(), repo_configs, repo_factory, mononoke)
        .await
        .context("Failed to initialize MegarepoApi")?;
    let session = SessionContainer::new_with_defaults(fb);
    let ctx = session.new_context(logger.clone(), matches.scuba_sample_builder());
    match sub_m.subcommand() {
        (LIST_CMD, Some(sub_m)) => handle_list(sub_m, ctx, megarepo).await?,
        (SHOW_CMD, Some(sub_m)) => handle_show(sub_m, ctx, megarepo).await?,
        (ABORT_CMD, Some(sub_m)) => handle_abort(sub_m, ctx, megarepo).await?,
        (REQUEUE_CMD, Some(sub_m)) => handle_requeue(sub_m, ctx, megarepo).await?,
        _ => return Err(SubcommandError::InvalidArgs),
    }
    Ok(())
}

async fn handle_list(
    args: &ArgMatches<'_>,
    ctx: CoreContext,
    megarepo: MegarepoApi,
) -> Result<(), Error> {
    let repos_and_queues = megarepo.all_async_method_request_queues(&ctx).await?;

    let lookback = value_t!(args.value_of(LOOKBACK_SECS), i64)?;

    let mut table = Table::new();
    table.set_titles(row![
        "Request id",
        "Method",
        "Status",
        "Target bookmark",
        "Source name (sync_changeset)",
        "Source Changeset (sync_changeset)"
    ]);
    for (repo_ids, queue) in repos_and_queues {
        let res = queue
            .list_requests(
                &ctx,
                &repo_ids,
                &[
                    RequestStatus::New,
                    RequestStatus::InProgress,
                    RequestStatus::Ready,
                    RequestStatus::Polled,
                ],
                Some(&Timestamp::from_timestamp_secs(
                    Timestamp::now().timestamp_seconds() - lookback,
                )),
            )
            .await?;
        for (req_id, entry, params) in res.into_iter() {
            let (source_name, changeset_id) = match params.thrift() {
                ThriftMegarepoAsynchronousRequestParams::megarepo_sync_changeset_params(params) => {
                    (
                        params.source_name.clone(),
                        ChangesetId::from_bytes(params.cs_id.clone())?.to_string(),
                    )
                }
                _ => ("".to_string(), "".to_string()),
            };
            table.add_row(row![
                req_id.0,
                req_id.1,
                entry.status,
                params.target()?.bookmark,
                &source_name,
                &changeset_id
            ]);
        }
    }
    table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
    table.printstd();

    Ok(())
}

async fn handle_show(
    args: &ArgMatches<'_>,
    ctx: CoreContext,
    megarepo: MegarepoApi,
) -> Result<(), Error> {
    let repos_and_queues = megarepo.all_async_method_request_queues(&ctx).await?;

    let row_id = value_t!(args.value_of(REQUEST_ID_ARG), u64)?;

    for (_repo_ids, queue) in repos_and_queues {
        if let Some((_request_id, entry, params, maybe_result)) =
            queue.get_request_by_id(&ctx, &RowId(row_id)).await?
        {
            println!(
                "Entry: {:?}\nParams: {:?}\nResult: {:?}",
                entry, params, maybe_result
            );
            return Ok(());
        }
    }
    Err(anyhow!("Request not found."))
}

async fn handle_abort(
    args: &ArgMatches<'_>,
    ctx: CoreContext,
    megarepo: MegarepoApi,
) -> Result<(), Error> {
    let repos_and_queues = megarepo.all_async_method_request_queues(&ctx).await?;

    let row_id = value_t!(args.value_of(REQUEST_ID_ARG), u64)?;

    for (_repo_ids, queue) in repos_and_queues {
        if let Some((request_id, _entry, params, maybe_result)) =
            queue.get_request_by_id(&ctx, &RowId(row_id)).await?
        {
            if maybe_result == None {
                let err = MegarepoError::InternalError(anyhow!("aborted from CLI!").into());
                let result: MegarepoAsynchronousRequestResult  = match params.thrift() {
                    ThriftMegarepoAsynchronousRequestParams::megarepo_sync_changeset_params(_) => {
                        MegarepoSyncChangesetResult::error(err.into()).into()
                    }
                    ThriftMegarepoAsynchronousRequestParams::megarepo_add_target_params(_) => {
                        MegarepoAddTargetResult::error(err.into()).into()
                    }
                    ThriftMegarepoAsynchronousRequestParams::megarepo_change_target_params(_) => {
                        MegarepoChangeTargetConfigResult::error(err.into()).into()
                    }
                    ThriftMegarepoAsynchronousRequestParams::megarepo_remerge_source_params(_) => {
                        MegarepoRemergeSourceResult::error(err.into()).into()
                    }
                    ThriftMegarepoAsynchronousRequestParams::megarepo_add_branching_target_params(_) => {
                        MegarepoAddBranchingTargetResult::error(err.into()).into()
                    }
                    _ => return Err(anyhow!("unknown request type!"))
                };
                queue.complete(&ctx, &request_id, result).await?;
            } else {
                return Err(anyhow!("Request already completed."));
            }
            return Ok(());
        }
    }
    Err(anyhow!("Request not found."))
}

async fn handle_requeue(
    args: &ArgMatches<'_>,
    ctx: CoreContext,
    megarepo: MegarepoApi,
) -> Result<(), Error> {
    let repos_and_queues = megarepo.all_async_method_request_queues(&ctx).await?;

    let row_id = value_t!(args.value_of(REQUEST_ID_ARG), u64)?;

    for (_repo_ids, queue) in repos_and_queues {
        if let Some((request_id, _entry, _params, _maybe_result)) =
            queue.get_request_by_id(&ctx, &RowId(row_id)).await?
        {
            queue.requeue(&ctx, request_id).await?;
            return Ok(());
        }
    }
    Err(anyhow!("Request not found."))
}
