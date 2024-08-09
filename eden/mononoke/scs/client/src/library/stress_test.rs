/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Benchmark commands.
use std::time::Duration;
use std::time::Instant;

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use futures::stream;
use futures::stream::StreamExt;
use tokio::time;

#[derive(clap::Parser)]
/// List the contents of a directory
pub(crate) struct StressArgs {
    /// Run in stress test mode
    #[clap(long = "stress")]
    pub(crate) enabled: bool,

    /// Number of times to run the command
    #[clap(long = "stress-count", default_value_t = 10000)]
    pub(crate) count: usize,

    /// How many requests per second to send in stress-test mode. Passing this argument will select the "paced"
    /// algorithm.
    #[clap(long = "stress-pace", default_value_t = 0, conflicts_with = "parallel")]
    pub(crate) pace: u64,

    /// Number of parallel commands to run
    #[clap(
        long = "stress-parallel",
        default_value_t = 100,
        conflicts_with = "pace"
    )]
    pub(crate) parallel: usize,
}

impl StressArgs {
    pub(crate) fn new_runner(
        &self,
        client_correlator: Option<String>,
    ) -> Box<dyn StressTestRunner> {
        if self.pace > 0 {
            Box::new(Paced {
                client_correlator,
                pace: self.pace,
                count: self.count,
            })
        } else {
            Box::new(Reckless {
                client_correlator,
                count: self.count,
                parallel: self.parallel,
            })
        }
    }
}

trait StressRunnerFn = Fn() -> futures::future::BoxFuture<'static, Result<(), Error>> + Send + Sync;

#[async_trait]
pub(crate) trait StressTestRunner {
    async fn run(
        &self,
        fun: Box<dyn StressRunnerFn>,
    ) -> Box<dyn Iterator<Item = Result<(), Error>>>;
}

struct Reckless {
    client_correlator: Option<String>,
    count: usize,
    parallel: usize,
}

#[async_trait]
impl StressTestRunner for Reckless {
    /// Run a function `count` times in parallel, as fast as possible, even if that
    /// means overloading the server.
    async fn run(
        &self,
        fun: Box<dyn StressRunnerFn>,
    ) -> Box<dyn Iterator<Item = Result<(), Error>>> {
        print_header(
            format!(
                "running stress test with count: {} parallel: {}",
                self.count, self.parallel,
            ),
            &self.client_correlator,
        );

        Box::new(
            stream::iter(0..self.count)
                .map(|_| fun())
                .buffer_unordered(self.parallel)
                .collect::<Vec<_>>()
                .await
                .into_iter(),
        )
    }
}

struct Paced {
    client_correlator: Option<String>,
    pace: u64,
    count: usize,
}

#[async_trait]
impl StressTestRunner for Paced {
    /// Run a function `count` times in parallel, as fast as possible, even if that
    /// means overloading the server.
    async fn run(
        &self,
        fun: Box<dyn StressRunnerFn>,
    ) -> Box<dyn Iterator<Item = Result<(), Error>>> {
        print_header(
            format!(
                "running paced stress test with pace: {}ms count: {}",
                self.pace, self.count,
            ),
            &self.client_correlator,
        );

        let interval = time::interval(Duration::from_millis(self.pace));

        let forever = stream::unfold(interval, |mut interval| async {
            interval.tick().await;
            let res = fun().await;
            Some((res, interval))
        });

        let _now = Instant::now();
        Box::new(
            forever
                .take(self.count)
                .collect::<Vec<_>>()
                .await
                .into_iter(),
        )
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
