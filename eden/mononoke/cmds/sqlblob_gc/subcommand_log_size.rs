/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{collections::HashMap, ops::Range};

use anyhow::Result;
use bytesize::ByteSize;
use clap::{App, Arg, ArgMatches, SubCommand};
use fbinit::FacebookInit;
use futures::stream::{self, StreamExt, TryStreamExt};
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;

use sqlblob::Sqlblob;

pub const LOG_SIZE: &str = "generation-size";
const ARG_SCUBA_TABLE: &str = "scuba-table";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(LOG_SIZE)
        .about("measure generation sizes")
        .arg(
            Arg::with_name(ARG_SCUBA_TABLE)
                .long(ARG_SCUBA_TABLE)
                .takes_value(true)
                .required(false)
                .help("Scuba table to log sizes to. If not specified, will print to stdout"),
        )
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
    fb: FacebookInit,
    _logger: Logger,
    sub_matches: &'_ ArgMatches<'_>,
    max_parallelism: usize,
    sqlblob: Sqlblob,
    shard_range: Range<usize>,
) -> Result<()> {
    let sizes: Vec<_> = shard_range
        .map(|shard| sqlblob.get_chunk_sizes_by_generation(shard))
        .collect();
    let sizes = stream::iter(sizes.into_iter())
        .buffer_unordered(max_parallelism)
        .try_fold(HashMap::new(), |mut acc, sizes| async move {
            for (gen, size) in sizes {
                *acc.entry(gen).or_insert(0u64) += size;
            }
            Ok(acc)
        })
        .await?;

    let scuba_sample_builder = MononokeScubaSampleBuilder::with_opt_table(
        fb,
        sub_matches.value_of(ARG_SCUBA_TABLE).map(String::from),
    );

    for (generation, size) in &sizes {
        let mut sample = scuba_sample_builder.clone();
        sample.add_opt("generation", *generation);
        sample.add("size", *size);
        sample.log();
    }

    if sub_matches.value_of(ARG_SCUBA_TABLE).is_none() {
        print_sizes(&sizes);
    }
    Ok(())
}
