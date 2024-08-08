/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Benchmark commands.
use anyhow::Error;
use anyhow::Result;
use futures::stream;
use futures::stream::StreamExt;

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

impl StressArgs {
    pub(crate) fn new_runner(&self, client_correlator: Option<String>) -> impl StressTestRunner {
        Reckless {
            client_correlator,
            count: self.count,
            parallel: self.parallel,
        }
    }
}

pub(crate) trait StressTestRunner {
    async fn run<F>(&self, fun: F) -> impl Iterator<Item = Result<(), Error>>
    where
        F: Fn() -> ::futures::future::BoxFuture<'static, Result<(), Error>>;
}

struct Reckless {
    client_correlator: Option<String>,
    count: usize,
    parallel: usize,
}

impl StressTestRunner for Reckless {
    /// Run a function `count` times in parallel, as fast as possible, even if that
    /// means overloading the server.
    async fn run<F>(&self, fun: F) -> impl Iterator<Item = Result<(), Error>>
    where
        F: Fn() -> ::futures::future::BoxFuture<'static, Result<(), Error>>,
    {
        print_header(
            format!(
                "running stress test with count: {} parallel: {}",
                self.count, self.parallel,
            ),
            &self.client_correlator,
        );

        stream::iter(0..self.count)
            .map(|_| fun())
            .buffer_unordered(self.parallel)
            .collect::<Vec<_>>()
            .await
            .into_iter()
    }
}

fn print_header(msg: String, client_correlator: &Option<String>) {
    println!(
        "{}.{}",
        msg,
        client_correlator
            .clone()
            .map_or("".to_string(), |c| format!(" client correlator: {}", c)),
    );
}
