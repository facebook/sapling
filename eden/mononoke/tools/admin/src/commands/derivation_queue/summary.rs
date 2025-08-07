/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::anyhow;
use clap::Args;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use mononoke_app::args::MultiDerivedDataArgs;
use mononoke_types::DerivableType;
use prettytable::Table;
use prettytable::cell;
use prettytable::row;
use repo_derivation_queues::DerivationQueueSummary;
use repo_derivation_queues::RepoDerivationQueuesRef;

use super::Repo;

#[derive(Args)]
pub struct SummaryArgs {
    /// Display the client info for each item in the queue
    #[clap(short, long)]
    client_info: bool,

    /// Limit the number of items to display.
    #[clap(short, long, default_value_t = 20)]
    limit: usize,

    /// Filter by derived data types.
    #[clap(flatten)]
    multi_derived_data_args: MultiDerivedDataArgs,

    /// Whether to output in JSON format.
    #[clap(long)]
    json: bool,
}

pub async fn summary(
    ctx: &CoreContext,
    repo: &Repo,
    config_name: &str,
    args: SummaryArgs,
) -> Result<()> {
    let derivation_queue = repo
        .repo_derivation_queues()
        .queue(config_name)
        .ok_or_else(|| anyhow!("Missing derivation queue for config {}", config_name))?;

    let summary = derivation_queue.summary(ctx).await?;

    let derived_data_types = args
        .multi_derived_data_args
        .resolve_types(derivation_queue.derived_data_manager().config())?;

    if args.json {
        print_json(ctx, args, summary).await?
    } else {
        print_table(ctx, args, summary, derived_data_types).await?;
    }

    Ok(())
}

async fn print_json(
    _ctx: &CoreContext,
    args: SummaryArgs,
    summary: DerivationQueueSummary<'_>,
) -> Result<()> {
    let items = summary
        .items
        .take(args.limit)
        .try_collect::<Vec<_>>()
        .await?;
    println!("{}", serde_json::to_string(&items)?);

    Ok(())
}

async fn print_table(
    _ctx: &CoreContext,
    args: SummaryArgs,
    summary: DerivationQueueSummary<'_>,
    derived_data_types: Vec<DerivableType>,
) -> Result<()> {
    let mut table = Table::new();

    let mut titles = row![
        "time in queue",
        "retry count",
        "type",
        "bubble",
        "head",
        "root"
    ];
    if args.client_info {
        titles.add_cell(cell!["client info"]);
    }
    table.set_titles(titles);

    println!("Number of items in the queue: {}", summary.queue_size);
    let mut item_stream = summary.items.take(args.limit);
    while let Some(result) = item_stream.next().await {
        let item = result?;
        let dd_type = item.derived_data_type();
        if derived_data_types.contains(&dd_type) {
            let timestamp = item
                .enqueue_timestamp()
                .ok_or_else(|| anyhow!("Missing enqueue timestamp"))?;
            let mut row = row![
                format!(
                    "{}s{}ms",
                    timestamp.since_seconds(),
                    timestamp.since_millis() % 1000
                ),
                item.retry_count(),
                dd_type,
                format!("{:?}", item.bubble_id()),
                item.head_cs_id(),
                item.root_cs_id(),
            ];
            if args.client_info {
                row.add_cell(cell![format!("{:?}", item.client_info())]);
            }
            table.add_row(row);
        }
    }
    table.set_format(*prettytable::format::consts::FORMAT_BOX_CHARS);
    table.printstd();

    Ok(())
}
