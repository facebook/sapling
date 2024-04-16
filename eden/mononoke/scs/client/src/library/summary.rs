/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Benchmark commands.
use std::io::Write;

use anyhow::Error;
use anyhow::Result;
use futures::stream;
use futures::stream::StreamExt;
use futures::Stream;
use itertools::Itertools;
use serde::Serialize;

use crate::render::Render;

#[derive(clap::Parser)]
/// List the contents of a directory
pub(crate) struct StressArgs {
    /// Run in stress test mode
    #[clap(long = "stress")]
    pub(crate) enabled: bool,
    /// Number of times to run the command
    #[clap(long = "stress-count", default_value_t = 10000)]
    pub(crate) count: usize,
    /// Number of parallel commands to run
    #[clap(long = "stress-parallel", default_value_t = 100)]
    pub(crate) parallel: usize,
}

#[derive(Serialize)]
pub(crate) struct SummaryOutput {
    result: String,
    count: usize,
}

impl Render for SummaryOutput {
    type Args = ();

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        write!(w, "{} times: {}\n", self.count, self.result)?;
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

/// Run a function `count` times in parallel, as fast as possible, even if that
/// means overloading the server.
pub(crate) async fn run_stress<F>(
    count: usize,
    parallel: usize,
    fun: F,
) -> impl Iterator<Item = Result<(), Error>>
where
    F: Fn() -> ::futures::future::BoxFuture<'static, Result<(), Error>>,
{
    stream::iter(0..count)
        .map(|_| fun())
        .buffer_unordered(parallel)
        .collect::<Vec<_>>()
        .await
        .into_iter()
}

pub(crate) fn summary_output(
    results: impl Iterator<Item = Result<(), Error>>,
) -> impl Stream<Item = Result<SummaryOutput>> {
    let ret = results
        .map(|res| match res {
            Ok(_) => "OK".to_string(),
            Err(e) => format!("{:?}", e),
        })
        .sorted()
        .counts()
        .into_iter()
        .map(|(key, count)| {
            Ok(SummaryOutput {
                result: key.clone(),
                count,
            })
        });
    stream::iter(ret)
}
