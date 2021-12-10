/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{collections::HashMap, ops::Range};

use anyhow::Result;
use bytesize::ByteSize;
use clap::{App, ArgMatches, SubCommand};
use futures::stream::{self, StreamExt, TryStreamExt};
use futures::TryFutureExt;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;

use sqlblob::Sqlblob;

pub const LOG_SIZE: &str = "generation-size";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(LOG_SIZE).about("measure generation sizes")
}

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

pub async fn subcommand_log_size(
    _logger: Logger,
    _sub_matches: &'_ ArgMatches<'_>,
    max_parallelism: usize,
    sqlblob: Sqlblob,
    shard_range: Range<usize>,
    scuba_sample_builder: MononokeScubaSampleBuilder,
) -> Result<()> {
    let shard_sizes: Vec<(usize, HashMap<Option<u64>, u64>)> = stream::iter(shard_range)
        .map(|shard| {
            sqlblob
                .get_chunk_sizes_by_generation(shard)
                .map_ok(move |gen_to_size| (shard, gen_to_size))
        })
        .buffer_unordered(max_parallelism)
        .try_collect::<Vec<(usize, HashMap<Option<u64>, u64>)>>()
        .await?;

    let mut size_summary = HashMap::new();
    for (shard, sizes) in shard_sizes {
        for (generation, size) in sizes.into_iter() {
            let mut sample = scuba_sample_builder.clone();
            sample.add("shard", shard);
            sample.add_opt("generation", generation);
            sample.add("size", size);
            sample.log();
            *size_summary.entry(generation).or_insert(0u64) += size;
        }
    }

    if scuba_sample_builder.is_discard() {
        print_sizes(&size_summary);
    }
    Ok(())
}
