/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use prettytable::cell;
use prettytable::row;
use prettytable::Table;
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

    println!("Number of items in the queue: {}", summary.items.len());
    for item in summary.items.into_iter().take(args.limit) {
        let timestamp = item
            .enqueue_timestamp()
            .ok_or_else(|| anyhow!("Missing enqueue timestamp"))?;
        let mut row = row![
            format!(
                "{}s{}ms",
                timestamp.since_seconds(),
                timestamp.since_millis() % 1000
            ),
            item.derived_data_type(),
            format!("{:?}", item.bubble_id()),
            item.head_cs_id(),
            item.root_cs_id(),
        ];
        if args.client_info {
            row.add_cell(cell![format!("{:?}", item.client_info())]);
        }
        table.add_row(row);
    }
    table.set_format(*prettytable::format::consts::FORMAT_BOX_CHARS);
    table.printstd();

    Ok(())
}
