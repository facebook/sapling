/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Global `sampling_profiler::Profiler` state for ease-of-use.
//!
//! - Handle application configuration logic to start profiling.
//! - Make nested profiler starts do nothing.
//! - Print the profiling result, or store it in a variable.
//!   By using `AtExit`, it ideally still print on Ctrl+C.

use std::cell::RefCell;
use std::io;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::Result;
use atexit::AtExit;
use configmodel::Config;
use configmodel::ConfigExt;
use fs_err as fs;
use parking_lot::RwLock;
use sampling_profiler::BacktraceCollector;
use sampling_profiler::Profiler;

thread_local! {
    static PROFILER: RefCell<Option<Profiler>> = RefCell::new(None);
}

/// Global profiling backtraces.
/// With profiling interval = 1 second, the estimated memory usage is a few MBs
/// per hour.
static BACKTRACE_COLLECTOR: LazyLock<Arc<RwLock<BacktraceCollector>>> =
    LazyLock::new(Default::default);

/// Global profiling output. Written by [`teardown_profiling`].
static PROFILING_SUMMARIES: LazyLock<RwLock<Vec<String>>> = LazyLock::new(Default::default);

/// Start global profiling for the current (persumably main) thread.
/// Returns `None` if profiling was already set up.
/// Returns a droppable object that will stop and output profiling result.
///
/// Explicit profiling config:
///
/// ```plain,ignore
/// [profiling]
/// enabled = false   # usually set by the --profile global flag
/// interval = 10ms
/// output =
/// ```
///
/// If explicit profiling is not enabled, always-on profiling will be used
/// instead:
///
/// ```plain,ignore
/// [profiling]
/// always-on-enabled = false
/// always-on-interval = 1s
/// ```
///
/// Dropping the returned value pushes the profiling summary to PROFILING_SUMMARIES.
///
/// The config names except for `enabled` are intentionally different from what
/// `profiling.py` uses so they can be configured separately.
pub fn setup_profiling(config: &dyn Config) -> Result<Option<AtExit>> {
    PROFILER.with_borrow_mut(|profiler| -> Result<Option<AtExit>> {
        if profiler.is_some() {
            // Already setup (ex. nested command runs)
            return Ok(None);
        }

        let section = "profiling";
        let prefix = ["", "always-on-"].into_iter().find(|prefix| {
            let name = format!("{prefix}enabled");
            config.get_or_default(section, &name).unwrap_or(false)
        });
        let prefix = match prefix {
            None => return Ok(None),
            Some(v) => v,
        };
        let output: Option<String> = if prefix == "" {
            let output = config.get_or_default::<String>("profiling", "output")?;
            Some(output)
        } else {
            None
        };

        let interval = config
            .get_or::<Duration>(section, &format!("{prefix}interval"), || {
                let millis = if prefix == "" { 10 } else { 1000 };
                Duration::from_millis(millis)
            })?
            .clamp(Duration::from_millis(2), Duration::from_hours(24));

        // Prepare `BacktraceCollector`.
        let footnote = format!("Duration 1 unit = Sampling interval = {interval:?}.");
        let collector = BACKTRACE_COLLECTOR.clone();
        *collector.write() = BacktraceCollector::default().with_footnote(footnote);

        // Attempt to initialize (at least part of) Python frame resolution.
        // If the Python interpreter is not initialized, this will not completely
        // enable the Python frame resolution. Python initialization logic should
        // call this function again for full initialization.
        backtrace_python::init();

        // Prepare `Profiler`. This starts profiling.
        *profiler = Profiler::new(
            interval,
            Box::new(move |bt| {
                let bt: Vec<String> = bt
                    .iter()
                    .rev()
                    .filter(|n| !is_frame_name_boring(n))
                    .cloned()
                    .collect();
                collector.write().push_backtrace(bt);
            }),
        )
        .ok();

        let at_exit = AtExit::new(Box::new(move || teardown_profiling(output)));
        tracing::debug!(?interval, "Profiler initialized");
        Ok(Some(at_exit))
    })
}

/// Calculate the ASCII summary for the currently ongoing profiling.
pub fn in_progress_profiling_summary() -> String {
    BACKTRACE_COLLECTOR.read().ascii_summary()
}

/// Get the ASCII summaries for completed profilings.
/// Intended to be used by the caller to combine it with other types of tracing info.
pub fn completed_profiling_summaries() -> Vec<String> {
    let mut summaries = Vec::new();
    std::mem::swap(&mut summaries, &mut PROFILING_SUMMARIES.write());
    summaries
}

fn is_frame_name_boring(name: &str) -> bool {
    name.contains("cpython[") || name == "__rust_try"
}

fn teardown_profiling(output: Option<String>) {
    PROFILER.with_borrow_mut(|p| {
        let p = p.take();
        if let Some(p) = p {
            // Stop profiling. Wait for backtraces to be collected.
            drop(p);

            let collector = BACKTRACE_COLLECTOR.clone();
            let summary = collector.read().ascii_summary();

            // Write to specified output.
            if let Some(output) = output {
                'write_output: {
                    let mut out: Box<dyn io::Write> = match output.as_str() {
                        // stderr
                        "" => match clidispatch::io::IO::main() {
                            Ok(io) => Box::new(io.error()) as Box<dyn io::Write>,
                            Err(_) => Box::new(io::stderr()) as Box<dyn io::Write>,
                        },
                        "blackbox" => {
                            // TODO: write to blackbox
                            break 'write_output;
                        }
                        // file
                        _ => {
                            let file = fs::OpenOptions::new()
                                .append(true)
                                .create(true)
                                .open(&output);
                            match file {
                                Ok(file) => Box::new(file),
                                Err(_) => break 'write_output,
                            }
                        }
                    };
                    let _ = write!(&mut out, "Profiling summary:\n{}", summary);
                }
            }

            // Always push to PROFILING_SUMMARIES.
            PROFILING_SUMMARIES.write().push(summary);
        }
    });
}
