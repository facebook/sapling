/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use clidispatch::ReqCtx;
use cliparser::define_flags;
use progress_model::IoTimeSeries;
use progress_model::ProgressBar;
use progress_model::Registry;

use crate::commands::ConfigSet;

define_flags! {
    pub struct StructuredProgressOpts {
        /// bar layout
        layout: String = "[b(n=Breath,p,a,s=10000),b(n=Pet)->[b(n=Dogs,t=10,s=500,u=wags),b(n=Cats,t=5,s=750,u=meows)],b(n=Eat)->[b(n=Sushi)->[b(n=Nigiri,t=5,s=500),b(n=Maki,t=2,s=1000)],b(n=Sake,p,t=2,s=3000)],b(n=Think,t=20,s=500,p)]",
    }
}

#[derive(Default, Debug)]
struct Bar {
    total: u64,
    name: String,
    sleep_ms: u64,
    unit: String,
    adhoc: bool,
    parallel: bool,
    children: Vec<Bar>,
}

pub fn run(ctx: ReqCtx<StructuredProgressOpts>, _config: &mut ConfigSet) -> Result<u8> {
    let bars = Bar::from_spec_list(&ctx.opts.layout)?.1;

    // Add a test io time series so we can see how things look.
    let time_series = IoTimeSeries::new("HTTP", "requests");
    time_series.populate_test_samples(1000, 1000, 1);
    Registry::main().register_io_time_series(&time_series);

    run_bars(bars).join().unwrap();

    Ok(0)
}

fn run_bars(bars: Vec<Bar>) -> JoinHandle<()> {
    let mut parallel = Vec::new();
    let mut serial: Vec<(Bar, Arc<ProgressBar>)> = Vec::new();
    let mut non_adhoc: Vec<Arc<ProgressBar>> = Vec::new();

    for bar in bars {
        let pb = progress_model::ProgressBarBuilder::new()
            .topic(bar.name.clone())
            .total(bar.total)
            .unit(bar.unit.clone())
            .adhoc(bar.adhoc)
            .thread_local_parent()
            .pending();

        if !pb.adhoc() {
            non_adhoc.push(pb.clone());
        }

        if bar.parallel {
            parallel.push(thread::spawn(move || run_bar(bar, pb)));
        } else {
            serial.push((bar, pb));
        }
    }

    thread::spawn(move || {
        for (bar, pb) in serial {
            run_bar(bar, pb);
        }

        for t in parallel {
            t.join().unwrap();
        }

        drop(non_adhoc);
    })
}

fn run_bar(bar: Bar, pb: Arc<ProgressBar>) {
    let _active = ProgressBar::push_active(pb.clone(), Registry::main());

    let children = run_bars(bar.children);

    if bar.total > 0 {
        for _ in 0..bar.total {
            pb.increase_position(1);
            thread::sleep(Duration::from_millis(bar.sleep_ms));
        }
    } else {
        thread::sleep(Duration::from_millis(bar.sleep_ms));
    }

    children.join().unwrap();
}

impl Bar {
    fn from_args(args: &str) -> Result<Self> {
        let mut b = Self::default();
        for v in args.split(',') {
            match v.split_once("=") {
                Some((k, v)) => match k {
                    "t" => b.total = v.parse()?,
                    "n" => b.name = v.to_string(),
                    "s" => b.sleep_ms = v.parse()?,
                    "u" => b.unit = v.to_string(),
                    _ => bail!("unknown key: {}", k),
                },
                None => match v {
                    "a" => b.adhoc = true,
                    "p" => b.parallel = true,
                    "" => {}
                    _ => bail!("unknown flag: {}", v),
                },
            }
        }
        Ok(b)
    }

    fn from_spec_list(mut spec: &str) -> Result<(&str, Vec<Self>)> {
        let mut bars: Vec<Bar> = Vec::new();

        spec = consume(spec, "[")?;

        while !spec.starts_with("]") {
            spec = consume(spec, "b(")?;
            let end_paren = spec.find(')').ok_or_else(|| anyhow!("expected )"))?;

            let mut bar = Bar::from_args(&spec[..end_paren])?;

            spec = &spec[end_paren + 1..];

            if let Ok(s) = consume(spec, "->") {
                let (s, children) = Self::from_spec_list(s)?;
                spec = s;
                bar.children = children;
            }

            if bar.name.is_empty() {
                let adhoc_count = bars.iter().filter(|b| b.adhoc).count();
                if bar.adhoc {
                    bar.name = format!("Adhoc {}", adhoc_count + 1);
                } else {
                    bar.name = format!("Step {}", (bars.len() - adhoc_count) + 1);
                }
            }

            bars.push(bar);

            if let Some(s) = spec.strip_prefix(',') {
                spec = s;
            } else {
                break;
            }
        }

        spec = consume(spec, "]")?;

        Ok((spec, bars))
    }
}

fn consume<'a>(s: &'a str, p: &str) -> Result<&'a str> {
    s.strip_prefix(p).ok_or_else(|| anyhow!("expected {}", p))
}

pub fn aliases() -> &'static str {
    "debugstructuredprogress"
}

pub fn doc() -> &'static str {
    "play around with look/behavior of structured progress bars

<layout> = [<spec>,<spec>...]

<spec> = b(<args>) | b(<args>)-><layout>

<args> is comma separated n=<name>, t=<total>, s=<sleep ms>, u=<unit>, a(dhoc), p(arallel)::

    Parallel causes bar to start immediately when parent starts. Non-parallel siblings are run serially.

    Adhoc causes bar to disappear as soon as it is done, and respects the `progress.delay` config.

    Ex: `b(n=Name,t=100,s=10,u=Unit,p)`
"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
