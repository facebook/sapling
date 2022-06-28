/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use crate::utils;
use crate::MononokeSQLBlobGCArgs;
use anyhow::Result;
use bytesize::ByteSize;
use clap::Parser;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::TryFutureExt;
use mononoke_app::MononokeApp;

/// measure generation sizes
#[derive(Parser)]
pub struct CommandArgs {}

fn print_sizes(sizes: &HashMap<Option<u64>, u64>) {
    let generations = {
        let mut keys: Vec<_> = sizes.keys().collect();
        keys.sort_unstable();
        keys
    };

    println!("Generation | Size");
    println!("-----------------");

    for generation in generations {
        let size = ByteSize::b(sizes[generation]);
        let generation = match generation {
            None => "NULL".to_string(),
            Some(g) => g.to_string(),
        };
        println!("{:>10} | {}", generation, size.to_string_as(true));
    }
}

pub async fn run(app: MononokeApp, _args: CommandArgs) -> Result<()> {
    let common_args: MononokeSQLBlobGCArgs = app.args()?;

    let max_parallelism: usize = common_args.scheduled_max;

    let (sqlblob, shard_range) = utils::get_sqlblob_and_shard_range(&app).await?;

    let scuba_sample_builder = app.environment().scuba_sample_builder.clone();

    let shard_sizes: Vec<(usize, HashMap<Option<u64>, (u64, u64)>)> = stream::iter(shard_range)
        .map(|shard| {
            sqlblob
                .get_chunk_sizes_by_generation(shard)
                .map_ok(move |gen_to_size| (shard, gen_to_size))
        })
        .buffer_unordered(max_parallelism)
        .try_collect::<Vec<(usize, HashMap<Option<u64>, (u64, u64)>)>>()
        .await?;

    let mut size_summary = HashMap::new();
    for (shard, sizes) in shard_sizes {
        for (generation, (size, chunk_id_count)) in sizes.into_iter() {
            let mut sample = scuba_sample_builder.clone();
            sample.add("shard", shard);
            sample.add_opt("generation", generation);
            sample.add("size", size);
            sample.add("chunk_id_count", chunk_id_count);
            sample.log();
            *size_summary.entry(generation).or_insert(0u64) += size;
        }
    }

    if scuba_sample_builder.is_discard() {
        print_sizes(&size_summary);
    }
    Ok(())
}
