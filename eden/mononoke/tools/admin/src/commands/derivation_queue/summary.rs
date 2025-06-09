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
use mononoke_app::args::MultiDerivedDataArgs;
use prettytable::Table;
use prettytable::cell;
use prettytable::row;
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

    let mut table = Table::new();

    let mut titles = row!["time in queue", "type", "bubble", "head", "root"];
    if args.client_info {
        titles.add_cell(cell!["client info"]);
    }
    table.set_titles(titles);

    let derived_data_types = args
        .multi_derived_data_args
        .resolve_types(derivation_queue.derived_data_manager().config())?;

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
