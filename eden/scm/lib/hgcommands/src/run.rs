/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{commands, HgPython};
use clidispatch::{dispatch, errors};
use failure::Fallible as Result;
use parking_lot::Mutex;
use std::env;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Write};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::dispatcher::{self, Dispatch};
use tracing::{span, Level};
use tracing_collector::{TracingCollector, TracingData};

/// Run a Rust or Python command.
///
/// Have side effect on `io` and return the command exit code.
pub fn run_command(args: Vec<String>, io: &mut clidispatch::io::IO) -> i32 {
    let now = SystemTime::now();

    // The chgserver does not want tracing or blackbox setup, or going through
    // the Rust command table. Bypass them.
    if args.get(1).map(|s| s.as_ref()) == Some("serve")
        && args.get(2).map(|s| s.as_ref()) == Some("--cmdserver")
        && args.get(3).map(|s| s.as_ref()) == Some("chgunix2")
    {
        return HgPython::new(&args).run_hg(args, io);
    }

    // This is intended to be "process start". "exec/hgmain" seems to be
    // a better place for it. However, chg makes it tricky. Because if hgmain
    // decides to use chg, then there is no way to figure out which `blackbox`
    // to write to, because the repo initialization logic happened in another
    // process (a forked chg server).
    //
    // Having "run_command" here will make it logged by the forked chg server,
    // which is a bit more desiable. Since run_command is very close to process
    // start, it should reflect the duration of the command relatively
    // accurately, at least for non-chg cases.
    log_start(args.clone(), now);

    let cwd = match current_dir(io) {
        Err(e) => {
            let _ = io.write_err(format!("abort: cannot get current directory: {}\n", e));
            return exitcode::IOERR;
        }
        Ok(dir) => dir,
    };

    let (_tracing_level, tracing_data) = setup_tracing();
    let span = span!(
        Level::INFO,
        "run_command",
        name = AsRef::<str>::as_ref(&args[1..args.len().min(64)].join(" ")),
        exitcode = "",
    );

    let exit_code = span.in_scope(|| {
        let table = commands::table();

        match dispatch::dispatch(&table, args[1..].to_vec(), io) {
            Ok(ret) => ret as i32,
            Err(err) => {
                let should_fallback = if err.downcast_ref::<errors::FallbackToPython>().is_some() {
                    true
                } else if err.downcast_ref::<errors::UnknownCommand>().is_some() {
                    // XXX: Right now the Rust command table does not have all Python
                    // commands. Therefore Rust "UnknownCommand" needs a fallback.
                    //
                    // Ideally the Rust command table has Python command information and
                    // there is no fallback path (ex. all commands are in Rust, and the
                    // Rust implementation might just call into Python cmdutil utilities).
                    true
                } else {
                    false
                };

                if !should_fallback {
                    errors::print_error(&err, io);
                    return 255;
                }

                // Change the current dir back to the original so it is not surprising to the Python
                // code.
                let _ = env::set_current_dir(cwd);

                HgPython::new(&args).run_hg(args, io)
            }
        }
    });

    span.record("exitcode", &exit_code);

    let _ = maybe_write_trace(io, &tracing_data);

    log_end(exit_code as u8, now, tracing_data);

    // Sync the blackbox before returning: this exit code is going to be used to process::exit(),
    // so we need to flush now.
    blackbox::sync();

    exit_code
}

/// Similar to `std::env::current_dir`. But does some extra things:
/// - Attempt to autofix issues when running under a typical shell (which
///   sets $PWD), and a directory is deleted and then recreated.
fn current_dir(io: &mut clidispatch::io::IO) -> io::Result<PathBuf> {
    let result = env::current_dir();
    if let Err(ref err) = result {
        match err.kind() {
            io::ErrorKind::NotConnected | io::ErrorKind::NotFound => {
                // For those errors, attempt to fix it by `cd $PWD`.
                // - NotConnected: edenfsctl stop; edenfsctl start
                // - NotFound: rmdir $PWD; mkdir $PWD
                if let Ok(pwd) = env::var("PWD") {
                    if env::set_current_dir(pwd).is_ok() {
                        let _ = io.write_err("(warning: the current directory was recreated; consider running 'cd $PWD' to fix your shell)\n");
                        return env::current_dir();
                    }
                }
            }
            _ => (),
        }
    }
    result
}

fn setup_tracing() -> (Level, Arc<Mutex<TracingData>>) {
    // Setup TracingData singleton (currently owned by pytracing).
    {
        let mut data = pytracing::DATA.lock();
        // Only recreate TracingData if pid has changed (ex. chgserver's case
        // where it forks and runs commands - we want to log to different
        // blackbox trace events).  This makes it possible to use multiple
        // `run()`s in a single process
        if data.process_id() != unsafe { libc::getpid() } as u64 {
            *data.deref_mut() = TracingData::new();
        }
    }
    let data = pytracing::DATA.clone();

    let level = std::env::var("EDENSCM_TRACE_LEVEL")
        .ok()
        .and_then(|s| Level::from_str(&s).ok())
        .unwrap_or(Level::INFO);
    let collector = TracingCollector::new(data.clone(), level.clone());
    let _ = tracing::subscriber::set_global_default(collector);

    (level, data)
}

fn maybe_write_trace(
    io: &mut clidispatch::io::IO,
    tracing_data: &Arc<Mutex<TracingData>>,
) -> Result<()> {
    // Ad-hoc environment variable: EDENSCM_TRACE_OUTPUT. A more standard way
    // to access the data is via the blackbox interface.
    // Write ASCII or TraceEvent JSON (or gzipped JSON) to the specified path.
    if let Ok(path) = std::env::var("EDENSCM_TRACE_OUTPUT") {
        // A hardcoded minimal duration (in microseconds).
        let data = tracing_data.lock();
        match write_trace(io, &path, &data) {
            Ok(_) => io.write_err(format!("(Trace was written to {})\n", &path))?,
            Err(err) => {
                io.write_err(format!("(Failed to write Trace to {}: {})\n", &path, &err))?
            }
        }
    }
    Ok(())
}

pub(crate) fn write_trace(
    io: &mut clidispatch::io::IO,
    path: &str,
    data: &TracingData,
) -> Result<()> {
    enum Format {
        ASCII,
        TraceEventJSON,
        TraceEventGzip,
        SpansJSON,
    }

    let format = if path.ends_with(".txt") {
        Format::ASCII
    } else if path.ends_with("spans.json") {
        Format::SpansJSON
    } else if path.ends_with(".json") {
        Format::TraceEventJSON
    } else if path.ends_with(".gz") {
        Format::TraceEventGzip
    } else {
        Format::ASCII
    };

    let mut out: Box<dyn Write> = if path == "-" || path.is_empty() {
        Box::new(&mut io.output)
    } else {
        Box::new(BufWriter::new(File::create(&path)?))
    };

    match format {
        Format::ASCII => {
            let mut ascii_opts = tracing_collector::model::AsciiOptions::default();
            ascii_opts.min_duration_parent_percentage_to_show = 80;
            ascii_opts.min_duration_micros_to_hide = 60000;
            out.write_all(data.ascii(&ascii_opts).as_bytes())?;
            out.flush()?;
        }
        Format::SpansJSON => {
            let spans = data.tree_spans();
            blackbox::serde_json::to_writer(&mut out, &spans)?;
            out.flush()?;
        }
        Format::TraceEventGzip => {
            let mut out = Box::new(flate2::write::GzEncoder::new(
                out,
                flate2::Compression::new(6), // 6 is the default value
            ));
            data.write_trace_event_json(&mut out, Default::default())?;
            out.finish()?.flush()?;
        }
        Format::TraceEventJSON => {
            data.write_trace_event_json(&mut out, Default::default())?;
            out.flush()?;
        }
    }

    Ok(())
}

fn log_start(args: Vec<String>, now: SystemTime) {
    let inside_test = is_inside_test();
    let (uid, pid, nice) = if inside_test {
        (0, 0, 0)
    } else {
        #[cfg(unix)]
        unsafe {
            (
                libc::getuid() as u32,
                libc::getpid() as u32,
                libc::nice(0) as i32,
            )
        }

        #[cfg(not(unix))]
        unsafe {
            // uid and nice are not aviailable on Windows.
            (0, libc::getpid() as u32, 0)
        }
    };

    blackbox::log(&blackbox::event::Event::Start {
        pid,
        uid,
        nice,
        args,
        timestamp_ms: epoch_ms(now),
    });

    let mut parent_names = Vec::new();
    let mut parent_pids = Vec::new();
    if !inside_test {
        let mut ppid = procinfo::parent_pid(0);
        while ppid != 0 {
            let name = procinfo::exe_name(ppid);
            parent_names.push(name);
            parent_pids.push(ppid);
            ppid = procinfo::parent_pid(ppid);
        }
    }
    blackbox::log(&blackbox::event::Event::ProcessTree {
        names: parent_names,
        pids: parent_pids,
    });
}

fn log_end(exit_code: u8, now: SystemTime, tracing_data: Arc<Mutex<TracingData>>) {
    let inside_test = is_inside_test();
    let duration_ms = if inside_test {
        0
    } else {
        match now.elapsed() {
            Ok(duration) => duration.as_millis() as u64,
            Err(_) => 0,
        }
    };
    let max_rss = if inside_test {
        0
    } else {
        procinfo::max_rss_bytes()
    };

    blackbox::log(&blackbox::event::Event::Finish {
        exit_code,
        max_rss,
        duration_ms,
        timestamp_ms: epoch_ms(now),
    });

    // Stop sending tracing events to subscribers. This prevents
    // deadlock in this scope.
    dispatcher::with_default(&Dispatch::none(), || {
        // Log tracing data.
        if let Ok(serialized_trace) = {
            let data = tracing_data.lock();
            // Note: if mincode::serialize wants to mutate tracing_data here,
            // it can deadlock if the dispatcher is not Dispatch::none().
            mincode::serialize(&data.deref())
        } {
            if let Ok(compressed) = zstd::stream::encode_all(&serialized_trace[..], 0) {
                let event = blackbox::event::Event::TracingData {
                    serialized: blackbox::event::Binary(compressed),
                };
                blackbox::log(&event);
            }
        }
        blackbox::sync();
    });
}

fn epoch_ms(time: SystemTime) -> u64 {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as u64,
        Err(_) => 0,
    }
}

fn is_inside_test() -> bool {
    std::env::var_os("TESTTMP").is_some()
}

// TODO: Replace this with the 'exitcode' crate once it's available.
mod exitcode {
    pub const IOERR: i32 = 74;
}
