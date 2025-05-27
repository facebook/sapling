/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io;
use std::io::BufWriter;
use std::io::Write;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use anyhow::Context;
use anyhow::Result;
use blackbox::serde_json;
use clidispatch::dispatch;
use clidispatch::dispatch::Dispatcher;
use clidispatch::errors;
use clidispatch::global_flags::HgGlobalOpts;
use clidispatch::io::IO;
use clidispatch::io::IsTty;
use commandserver::ipc::Server;
use configloader::config::ConfigSet;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::Text;
use parking_lot::Mutex;
use progress_model::Registry;
use repo::repo::Repo;
use testutil::failpoint;
use tracing::Level;
use tracing::dispatcher;
use tracing::dispatcher::Dispatch;
use tracing::metadata::LevelFilter;
use tracing_collector::TracingData;
use tracing_sampler::SamplingLayer;
use tracing_subscriber::Layer;
use tracing_subscriber::fmt::Layer as FmtLayer;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;

use crate::HgPython;
use crate::commands;

/// Run a Rust or Python command.
///
/// Have side effect on `io` and return the command exit code.
pub fn run_command(args: Vec<String>, io: &IO) -> i32 {
    let start_time = StartTime::now();
    let start_blocked = io.time_interval().total_blocked_ms();

    // Ensure HgPython can initialize.
    crate::init();

    // The pfcserver or commandserver do not want tracing or blackbox or ctrlc setup,
    // or going through the Rust command table. Bypass them.
    if let Some(arg1) = args.get(1).map(|s| s.as_ref()) {
        match arg1 {
            "start-pfc-server" => {
                let config: Arc<dyn Config> = Arc::new(ConfigSet::new().named("pfc-server"));
                return HgPython::new(&args).run_hg(args, io, &config, false);
            }
            "start-commandserver" => {
                commandserver_serve(&args, io);
                return 0;
            }
            _ => {}
        }
    }

    // Initialize NodeIpc:
    // - Before spawning threads, since unsetenv (3) is MT-unsafe.
    // - After pfc-server, since we don't pfc-server to consume the IPC.
    // - Before debugpython, since it might be useful for Python logic.
    setup_nodeipc();

    // Skip initialization for debugpython. Make it closer to vanilla Python.
    if args.get(1).map(|s| s.as_str()) == Some("debugpython") {
        // naive command-line parsing: strip "--".
        let rest_args = if args.get(2).map(|s| s.as_str()) == Some("--") {
            &args[3..]
        } else {
            &args[2..]
        };
        let args: Vec<String> = std::iter::once("hgpython".to_string())
            .chain(rest_args.iter().cloned())
            .collect();
        let mut hgpython = HgPython::new(&args);
        constructors::init();
        return hgpython.run_python(&args, io) as i32;
    }

    // Extra initialization based on global flags.
    let global_opts = dispatch::parse_global_opts(&args[1..]).ok();

    // Setup tracing early since "log_start" will use it immediately.
    // The tracing clock starts ticking from here.
    let tracing_data = match setup_tracing(&global_opts, io) {
        Err(_) => {
            // With our current architecture it is common to see this path in our tests due to
            // trying to set a global collector a second time. Ignore the error and return some
            // dummy values. FIXME!
            Arc::new(Mutex::new(TracingData::new()))
        }
        Ok(res) => res,
    };

    // Do important finalization tasks (even when ctrl-C'd).
    setup_atexit(start_time);

    setup_ctrlc();

    let scenario = failpoint::setup_fail_points();
    constructors::init();

    // This is intended to be "process start". "exec/hgmain" seems to be
    // a better place for it. However, chg makes it tricky. Because if hgmain
    // decides to use chg, then there is no way to figure out which `blackbox`
    // to write to, because the repo initialization logic happened in another
    // process (a forked chg server).
    //
    // Having "run_command" here will make it logged by the forked chg server,
    // which is a bit more desirable. Since run_command is very close to process
    // start, it should reflect the duration of the command relatively
    // accurately, at least for non-chg cases.
    let span = log_start(args.clone(), start_time);

    // Ad-hoc environment variable: EDENSCM_TRACE_OUTPUT. A more standard way
    // to access the data is via the blackbox interface.
    let trace_output_path = match identity::debug_env_var("TRACE_OUTPUT") {
        Some((var_name, var_value)) => {
            // Unset environment variable so processes forked by this command
            // wouldn't rewrite the trace.
            // TODO: Audit that the environment access only happens in single-threaded code.
            unsafe { std::env::remove_var(var_name) };
            Some(var_value)
        }
        None => None,
    };

    let in_scope = Arc::new(()); // Used to tell progress rendering thread to stop.

    metrics_render::init_from_env(Arc::downgrade(&in_scope));

    let exit_code = (|| {
        let cwd = match current_dir(io) {
            Err(e) => {
                let _ = io.write_err(format!("abort: cannot get current directory: {}\n", e));
                return exitcode::IOERR;
            }
            Ok(dir) => dir,
        };

        match dispatch::Dispatcher::from_args(args.clone()) {
            Ok(dispatcher) => {
                let _guard = span.enter();

                sampling::init(dispatcher.config());

                dispatch_command(io, dispatcher, cwd, Arc::downgrade(&in_scope), start_time)
            }
            Err(err) => {
                errors::print_error(
                    &err,
                    io,
                    global_opts.as_ref().map_or(true, |opts| opts.traceback),
                );
                255
            }
        }
    })();

    drop(in_scope);

    let _ = maybe_write_trace(io, &tracing_data, trace_output_path);

    log_end(io, exit_code as u8, start_blocked, start_time, tracing_data);

    // Sync the blackbox before returning: this exit code is going to be used to process::exit(),
    // so we need to flush now.
    blackbox::sync();

    if let Some(scenario) = scenario {
        failpoint::teardown_fail_points(scenario);
    }

    crate::deinit();

    exit_code
}

fn dispatch_command(
    io: &IO,
    mut dispatcher: Dispatcher,
    cwd: PathBuf,
    in_scope: Weak<()>,
    start_time: StartTime,
) -> i32 {
    log_repo_path_and_exe_version(dispatcher.repo());

    if let Some(repo) = dispatcher.repo() {
        tracing::info!(target: "symlink_info",
                       symlinks_enabled=cfg!(unix) || repo.requirements.contains("windowssymlinks"));
        let _ = sampling::log!(
            target: "repo_info",
            repo_requirements = {
                let reqs1 = repo.requirements.to_set();
                let reqs2 = repo.store_requirements.to_set();
                let mut reqs: Vec<String> = reqs1.into_iter().chain(reqs2.into_iter()).collect();
                reqs.sort_unstable();
                reqs
            }
        );
    }

    let run_logger =
        match runlog::Logger::from_repo(dispatcher.repo(), dispatcher.args()[1..].to_vec()) {
            Ok(logger) => Some(logger),
            Err(err) => {
                let _ = io.write_err(format!("Error creating runlogger: {}\n", err));
                None
            }
        };

    setup_http(dispatcher.global_opts());

    let _ = spawn_progress_thread(
        dispatcher.config(),
        dispatcher.global_opts(),
        io,
        run_logger.clone(),
        in_scope,
    );

    let table = commands::table();

    let (command, dispatch_res) = dispatcher.run_command(&table, io);

    let config = dispatcher.config();

    let mut fell_back = false;
    let exit_code = match dispatch_res
        .map_err(|err| errors::triage_error(config, err, command.map(|c| c.main_alias())))
    {
        Ok(exit_code) => exit_code as i32,
        Err(err) => 'fallback: {
            let should_fallback = err.is::<errors::FallbackToPython>() ||
                // XXX: Right now the Rust command table does not have all Python
                // commands. Therefore Rust "UnknownCommand" needs a fallback.
                //
                // Ideally the Rust command table has Python command information and
                // there is no fallback path (ex. all commands are in Rust, and the
                // Rust implementation might just call into Python cmdutil utilities).
                err.is::<errors::UnknownCommand>();
            let failed_fallback = err.is::<errors::FailedFallbackToPython>();

            if failed_fallback {
                197
            } else if should_fallback {
                tracing::debug!(?err, "falling back to python");

                fell_back = true;
                // Change the current dir back to the original so it is not surprising to the Python
                // code.
                let _ = env::set_current_dir(cwd);

                if !IS_COMMANDSERVER.load(Ordering::Acquire)
                    && config
                        .get_or_default::<bool>("commandserver", "enabled")
                        .unwrap_or_default()
                {
                    // Attempt to connect to an existing command server.
                    let args = dispatcher.args();
                    if let Ok(ret) =
                        commandserver::client::run_via_commandserver(args.to_vec(), config)
                    {
                        break 'fallback ret;
                    }
                }

                // Init interpreter with unmodified args. This is so `sys.argv` in Python
                // reflects what the user actually typed (we muck with the args in Rust
                // dispatch).
                let mut interp = HgPython::new(dispatcher.orig_args());
                if dispatcher.global_opts().trace {
                    // Error is not fatal.
                    let _ = interp.setup_tracing("*".into());
                }

                let already_ran_pre_hooks = err.is::<errors::FallbackToPython>();

                interp.run_hg(
                    dispatcher.args().to_vec(),
                    io,
                    config,
                    already_ran_pre_hooks,
                )
            } else {
                errors::print_error(&err, io, dispatcher.global_opts().traceback);
                errors::upload_traceback(&err, start_time.epoch_ms());
                255
            }
        }
    };

    if !fell_back {
        if let Err(err) = io.wait_pager().context("error flushing command output") {
            errors::print_error(&err, io, dispatcher.global_opts().traceback);
            return 255;
        }
    }

    if let Err(err) = io.disable_progress(true) {
        tracing::warn!(?err, "error clearing progress at end of command");
    }

    // Clean up progress models.
    Registry::main().remove_orphan_models();

    if let Some(rl) = &run_logger {
        // Retry a couple times on Windows since this will fail if someone is
        // reading the file, and it is relatively important to write the final
        // runlog entry since it contains the exit code and exit time.
        let tries = if cfg!(windows) { 3 } else { 1 };
        for i in 0..tries {
            if i > 0 {
                thread::sleep(Duration::from_millis(1));
            }

            match rl.close(exit_code) {
                Ok(()) => break,
                Err(err) => {
                    if i == tries - 1 {
                        tracing::error!(target: "runlog", ?err, "error closing runlog")
                    } else {
                        tracing::warn!(target: "runlog", ?err, "error closing runlog")
                    }
                }
            };
        }
    }

    if let Err(err) = log_perftrace(io, config, start_time) {
        tracing::error!(?err, "error logging perftrace");
    }
    if let Err(err) = log_metrics(io, config) {
        tracing::error!(?err, "error printing metrics");
    }

    exit_code
}

/// Similar to `std::env::current_dir`. But does some extra things:
/// - Attempt to autofix issues when running under a typical shell (which
///   sets $PWD), and a directory is deleted and then recreated.
fn current_dir(io: &IO) -> io::Result<PathBuf> {
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
            _ => {}
        }
    }
    result
}

/// Make tracing write logs to `io` if `LOG` environment is set.
/// Return `true` if it is set, or `false` if nothing happens.
///
/// `collector` is used to integrate with the `TracingCollector`,
/// which can integrate with Python via bindings.
fn setup_tracing_io(
    io: &IO,
    collector: Option<tracing_collector::TracingCollector>,
) -> Result<bool> {
    let is_test = is_inside_test();
    let mut env_filter_dirs: Option<String> = identity::debug_env_var("LOG").map(|v| v.1);

    // Ensure EnvFilter is used in tests so it can be changed on the fly.
    if is_test && env_filter_dirs.is_none() {
        env_filter_dirs = Some(String::new());
    }

    if let Some(dirs) = env_filter_dirs {
        // Apply "reload" side effects first.
        let error = io.error();
        let can_color = error.can_color();
        tracing_reload::update_writer(Box::new(error));
        tracing_reload::update_env_filter_directives(&dirs)?;

        // This might error out if called 2nd time per process.
        let env_filter = tracing_reload::reloadable_env_filter()?;

        let env_logger = FmtLayer::new()
            .with_span_events(FmtSpan::ACTIVE)
            .with_ansi(can_color)
            .with_writer(tracing_reload::reloadable_writer);
        if is_test {
            // In tests, disable color and timestamps for cleaner output.
            let env_logger = env_logger.without_time().with_ansi(false);
            match collector {
                None => {
                    let subscriber = tracing_subscriber::Registry::default()
                        .with(env_logger.with_filter(env_filter))
                        .with(SamplingLayer::new());
                    tracing::subscriber::set_global_default(subscriber)?;
                }
                Some(collector) => {
                    let subscriber = tracing_subscriber::Registry::default()
                        .with(collector.and_then(env_logger).with_filter(env_filter))
                        .with(SamplingLayer::new());
                    tracing::subscriber::set_global_default(subscriber)?;
                }
            };
        } else {
            match collector {
                None => {
                    let subscriber = tracing_subscriber::Registry::default()
                        .with(env_logger.with_filter(env_filter))
                        .with(SamplingLayer::new());
                    tracing::subscriber::set_global_default(subscriber)?;
                }
                Some(collector) => {
                    let subscriber = tracing_subscriber::Registry::default()
                        .with(collector.and_then(env_logger).with_filter(env_filter))
                        .with(SamplingLayer::new());
                    tracing::subscriber::set_global_default(subscriber)?;
                }
            }
        }
        Ok(true)
    } else {
        Ok(false)
    }
}

fn setup_tracing(global_opts: &Option<HgGlobalOpts>, io: &IO) -> Result<Arc<Mutex<TracingData>>> {
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

    let collector = tracing_collector::TracingCollector::new(data.clone());
    if !setup_tracing_io(io, Some(collector))? {
        let level = identity::debug_env_var("TRACE_LEVEL")
            .map(|v| v.1)
            .and_then(|s| Level::from_str(&s).ok())
            .unwrap_or_else(|| {
                if let Some(opts) = global_opts {
                    if opts.trace {
                        return Level::DEBUG;
                    }
                }
                Level::INFO
            });

        let collector = tracing_collector::TracingCollector::new(data.clone());
        let subscriber = tracing_subscriber::Registry::default()
            .with(collector.with_filter::<LevelFilter>(level.into()))
            .with(SamplingLayer::new());
        tracing::subscriber::set_global_default(subscriber)?;
    }

    Ok(data)
}

fn spawn_progress_thread(
    config: &dyn Config,
    global_opts: &HgGlobalOpts,
    io: &IO,
    run_logger: Option<Arc<runlog::Logger>>,
    in_scope: Weak<()>,
) -> Result<()> {
    // See 'hg help config.progress' for the config options.
    let mut disable_rendering = false;

    if config.get_or("progress", "disable", || false)? {
        disable_rendering = true;
    }

    if global_opts.quiet || hgplain::is_plain(Some("progress")) {
        disable_rendering = true;
    }

    let renderer_name = config.get_or_default::<String>("progress", "renderer")?;
    if renderer_name == "none" {
        disable_rendering = true;
    }

    let assume_tty = config.get_or("progress", "assume-tty", || false)?;
    if !assume_tty && !io.error().is_tty() && renderer_name != "nodeipc" {
        disable_rendering = true;
    }

    let render_function = match renderer_name.as_str() {
        "structured" => progress_render::structured::render,
        "nodeipc" => progress_render::nodeipc::render,
        _ => progress_render::simple::render,
    };

    let interval = Duration::from_secs_f64(config.get_or("progress", "refresh", || 0.1)?)
        .max(Duration::from_millis(50));

    // lockstep is used by tests to control progress rendering run loop.
    let lockstep = config.get_or("progress", "lockstep", || false)?;

    // Limit how often we write runlog. This config knob is primarily for tests to lower.
    let runlog_interval =
        Duration::from_secs_f64(config.get_or("runlog", "progress-refresh", || 0.5)?).max(interval);

    let progress = io.progress();

    let (term_width, term_height) = progress.term_size();
    let mut config = progress_render::RenderingConfig {
        delay: Duration::from_secs_f64(config.get_or("progress", "delay", || 3.0)?),
        term_width,
        term_height,
        ..Default::default()
    };

    let registry = Registry::main();

    hg_http::enable_progress_reporting();

    // Not fatal if we cannot spawn the progress rendering thread.
    let thread_name = "rust-progress".to_string();
    let _ = thread::Builder::new().name(thread_name).spawn(move || {
        let mut last_changes = Vec::new();
        let mut last_runlog_time: Option<Instant> = None;

        while Weak::upgrade(&in_scope).is_some() {
            let now = Instant::now();

            if lockstep {
                registry.wait();
            }

            registry.remove_orphan_progress_bar();

            if !disable_rendering {
                let (term_width, term_height) = progress.term_size();
                config.term_width = term_width;
                config.term_height = term_height;

                let changes = (render_function)(registry, &config);
                if changes != last_changes {
                    // This might block (so we use a thread, not an async task)
                    let _ = progress.set(&changes);
                    last_changes = changes;
                }
            }

            if let Some(run_logger) = &run_logger {
                if last_runlog_time.map_or(true, |i| now - i >= runlog_interval) {
                    let progress = registry
                        .list_progress_bar()
                        .into_iter()
                        .map(runlog::Progress::new)
                        .collect();

                    if let Err(err) = run_logger.update_progress(progress) {
                        tracing::warn!(target: "runlog", ?err, "error updating runlog progress");
                    }

                    last_runlog_time = Some(now);
                }
            }

            if !lockstep {
                thread::sleep(interval);
            }
        }
    });

    Ok(())
}

fn maybe_write_trace(
    io: &IO,
    tracing_data: &Arc<Mutex<TracingData>>,
    path: Option<String>,
) -> Result<()> {
    // Write ASCII or TraceEvent JSON (or gzipped JSON) to the specified path.
    if let Some(path) = path {
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

pub(crate) fn write_trace(io: &IO, path: &str, data: &TracingData) -> Result<()> {
    enum Format {
        Ascii,
        TraceEventJSON,
        TraceEventGzip,
        SpansJSON,
    }

    let format = if path.ends_with(".txt") {
        Format::Ascii
    } else if path.ends_with("spans.json") {
        Format::SpansJSON
    } else if path.ends_with(".json") {
        Format::TraceEventJSON
    } else if path.ends_with(".gz") {
        Format::TraceEventGzip
    } else {
        Format::Ascii
    };

    let mut out: Box<dyn Write> = if path == "-" || path.is_empty() {
        Box::new(io.error())
    } else {
        Box::new(BufWriter::new(File::create(path)?))
    };

    match format {
        Format::Ascii => {
            let mut ascii_opts = tracing_collector::model::AsciiOptions::default();
            ascii_opts.min_duration_parent_percentage_to_show = 10;
            ascii_opts.min_duration_micros_to_hide = 100000;
            out.write_all(data.ascii(&ascii_opts).as_bytes())?;
            out.flush()?;
        }
        Format::SpansJSON => {
            let spans = data.tree_spans::<&str>();
            serde_json::to_writer(&mut out, &spans)?;
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

fn log_start(args: Vec<String>, now: StartTime) -> tracing::Span {
    let inside_test = is_inside_test();
    let (uid, pid, nice) = if inside_test {
        (0, 0, 0)
    } else {
        #[cfg(unix)]
        unsafe {
            (libc::getuid(), libc::getpid() as u32, libc::nice(0))
        }

        #[cfg(not(unix))]
        unsafe {
            // uid and nice are not aviailable on Windows.
            (0, libc::getpid() as u32, 0)
        }
    };

    if let Some((_, tags)) = identity::debug_env_var("BLACKBOX_TAGS") {
        tracing::info!(name = "blackbox_tags", tags = AsRef::<str>::as_ref(&tags));
        let names: Vec<String> = tags.split_whitespace().map(ToString::to_string).collect();
        blackbox::log(&blackbox::event::Event::Tags { names });
    }

    let mut parent_names = Vec::new();
    let mut parent_pids = Vec::new();
    // On Windows, getting the ppid and exe name requires `CreateToolhelp32Snapshot`,
    // which can take hundreds of milliseconds. So we skip doing that here.
    if !inside_test && !cfg!(windows) {
        let mut ppid = procinfo::parent_pid(0);
        // In theory, the OS should not report a cyclic process graph (ex. pid 1
        // has parent pid = 1). Practically `parent_pids` takes snapshots
        // every time on Windows (unnecessarily) and is subject to races. Be
        // extra careful here so the loop wouldn't be infinite.
        while ppid != 0 && parent_pids.len() < 16 && !parent_pids.contains(&ppid) {
            let name = procinfo::exe_name(ppid);
            parent_names.push(name);
            parent_pids.push(ppid);
            ppid = procinfo::parent_pid(ppid);
        }
    }

    let span = tracing::info_span!("run");

    blackbox::log(&blackbox::event::Event::Start {
        pid,
        uid,
        nice,
        args,
        timestamp_ms: now.epoch_ms(),
    });

    blackbox::log(&blackbox::event::Event::ProcessTree {
        names: parent_names,
        pids: parent_pids,
    });

    span
}

fn log_end(
    io: &IO,
    exit_code: u8,
    start_blocked: u64,
    start_time: StartTime,
    tracing_data: Arc<Mutex<TracingData>>,
) {
    let inside_test = is_inside_test();
    let duration_ms = if inside_test {
        0
    } else {
        start_time.elapsed().as_millis() as u64
    };
    let max_rss = if inside_test {
        0
    } else {
        procinfo::max_rss_bytes()
    };
    let total_blocked_ms = if inside_test {
        0
    } else {
        io.time_interval().total_blocked_ms() - start_blocked
    };

    if tracing::enabled!(target: "commands::run::blocked", Level::DEBUG) {
        let interval = io.time_interval();
        let tags = interval.list_tags();
        for tag in tags {
            let tag: &str = tag.as_ref();
            let time = interval.tagged_blocked_ms(tag);
            tracing::debug!(target: "commands::run::blocked", tag=tag, time=time, "blocked tag");
        }
        tracing::debug!(target: "commands::run::blocked", total=total_blocked_ms, start=start_blocked, "blocked total");
    }

    tracing::info!(
        target: "command_info",
        exit_code=exit_code,
        max_rss=max_rss,
        total_blocked_ms=total_blocked_ms,
        is_plain=hgplain::is_plain(None),
    );

    blackbox::log(&blackbox::event::Event::Finish {
        exit_code,
        max_rss,
        duration_ms,
        timestamp_ms: start_time.epoch_ms(),
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

#[derive(Copy, Clone)]
struct StartTime {
    t: SystemTime,
    i: Instant,
}

impl StartTime {
    pub fn now() -> Self {
        Self {
            t: SystemTime::now(),
            i: Instant::now(),
        }
    }

    pub fn epoch_ms(&self) -> u64 {
        match self.t.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(duration) => duration.as_millis() as u64,
            Err(_) => 0,
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.i.elapsed()
    }
}

fn is_inside_test() -> bool {
    std::env::var_os("TESTTMP").is_some()
}

fn log_repo_path_and_exe_version(repo: Option<&Repo>) {
    // The "version" and "repo" fields are consumed by telemetry.
    if let Some(repo) = repo {
        let config = repo.config();
        let opt_path_default: std::result::Result<Option<String>, _> =
            config.get_or_default("paths", "default");
        if let Ok(Some(path_default)) = opt_path_default {
            if let Some(repo_name) = repourl::repo_name_from_url(config, &path_default) {
                tracing::info!(
                    target: "command_info",
                    version = version::VERSION,
                    repo = repo_name.as_str(),
                );
                return;
            }
        }
    }
    tracing::info!(target: "command_info", version = version::VERSION);
}

fn log_perftrace(io: &IO, config: &dyn Config, start_time: StartTime) -> Result<()> {
    if let Some(threshold) = config.get_opt::<Duration>("tracing", "threshold")? {
        let elapsed = start_time.elapsed();
        if elapsed >= threshold {
            let key = format!(
                "flat/perftrace-{}-{}-{}",
                hostname::get()?.to_string_lossy(),
                std::process::id(),
                (start_time.epoch_ms() as f64) / 1e3,
            );

            let mut ascii_opts = tracing_collector::model::AsciiOptions::default();

            // Minimum resolution = 1% of duration.
            ascii_opts.min_duration_micros_to_hide = (elapsed.as_micros() / 100) as u64;

            let output = pytracing::DATA.lock().ascii(&ascii_opts);

            tracing::info!(target: "perftrace", key=key.as_str(), payload=output.as_str(), "Trace:\n{}\n", output);
            tracing::info!(target: "perftracekey", perftracekey=key.as_str(), "Trace key:\n{}\n", key);

            if config.get_or_default("tracing", "stderr")? {
                let _ = write!(io.error(), "{}\n", output);
            }
        }
    }

    Ok(())
}

fn log_metrics(io: &IO, config: &dyn Config) -> Result<()> {
    let mut metrics = hg_metrics::summarize();

    // Mix in counters from the "metrics" crate.
    metrics.extend(
        ::metrics::Registry::global()
            .counters()
            .into_iter()
            .filter_map(|(n, c)| {
                if c.is_gauge() || c.value() == 0 {
                    None
                } else {
                    Some((n.to_string(), c.value() as u64))
                }
            }),
    );

    if metrics.is_empty() {
        return Ok(());
    }

    // Log counters to "sampling" file.
    sampling::append_sample_map("metrics", &metrics);

    // Empty value means print everything.
    let prefixes: Option<Vec<Text>> = config.get_opt("devel", "print-metrics")?;
    let skip_prefixes: Vec<Text> = config.get_or_default("devel", "skip-metrics")?;

    let mut nested_counters = NestedCounters::Map(BTreeMap::new());

    let mut keys = metrics.keys().collect::<Vec<_>>();
    keys.sort();
    for key in keys {
        let value = *metrics.get(key).unwrap();

        nested_counters.insert(key, value);

        if skip_prefixes
            .iter()
            .any(|prefix| key.starts_with(prefix.as_ref()))
        {
            continue;
        }

        if prefixes.as_ref().is_some_and(|prefixes| {
            prefixes.is_empty()
                || prefixes
                    .iter()
                    .any(|prefix| key.starts_with(prefix.as_ref()))
        }) {
            writeln!(io.error(), "{key}: {}", value)?;
        }
    }

    blackbox::log(&blackbox::event::Event::LegacyLog {
        service: "metrics".to_string(),
        msg: serde_json::to_string(&HashMap::from([("metrics", nested_counters)]))?,
        opts: Default::default(),
    });

    Ok(())
}

// Type for transforming {foo.bar.baz: 123} into {foo: {bar: {baz: 123}}}.
#[derive(serde::Serialize)]
#[serde(untagged)]
enum NestedCounters<'a> {
    Value(u64),
    Map(BTreeMap<&'a str, NestedCounters<'a>>),
}

impl<'a> NestedCounters<'a> {
    fn insert(&mut self, key: &'a str, value: u64) {
        // collision between a value and map will drop value and use map
        // e.g. {"foo" => 123, "foo.bar" => 456} becomes {"foo" => {"bar" => 456}}

        match self {
            Self::Value(_) => {
                *self = Self::Map(BTreeMap::new());
                self.insert(key, value)
            }
            Self::Map(map) => {
                if let Some(idx) = key.find(['.', '_', '/']) {
                    let (name, rest) = (&key[..idx], &key[idx + 1..]);
                    map.entry(name)
                        .or_insert_with(|| Self::Map(BTreeMap::new()))
                        .insert(rest, value);
                } else if !map.contains_key(key) {
                    map.insert(key, Self::Value(value));
                }
            }
        }
    }
}

// TODO: Replace this with the 'exitcode' crate once it's available.
mod exitcode {
    pub const IOERR: i32 = 74;
}

fn setup_http(global_opts: &HgGlobalOpts) {
    if global_opts.insecure {
        hg_http::enable_insecure_mode();
    }
}

fn setup_atexit(start_time: StartTime) {
    atexit::AtExit::new(Box::new(move || {
        let duration_ms = start_time.elapsed().as_millis() as u64;

        tracing::debug!(target: "measuredtimes", command_duration=duration_ms);

        // Make extra sure our metrics are written out.
        sampling::flush();
    }))
    .named("flush sampling".into())
    .queued();
}

fn setup_ctrlc() {
    // ctrlc with the "termination" feature would register SIGINT, SIGTERM and
    // SIGHUP handlers.
    //
    // If you change this function, ensure to check Ctrl+C and SIGTERM works for
    // these cases:
    // - Python, native code released GIL: dbsh -c 'b.sleep(1000, False)'
    // - Python, native code took GIL: dbsh -c 'b.sleep(1000, True)'
    // - Rust: debugracyoutput
    // - Pager, block on `write`: log --pager=always
    // - Pager, block on `wait`: log -r . --pager=always --config pager.interface=full
    let _ = ctrlc::set_handler(|| {
        // Minimal cleanup then just exit. Our main storage (indexedlog,
        // metalog) is SIGKILL-safe, if "finally" (Python) or "Drop" (Rust) does
        // not run, it won't corrupt the repo data.

        tracing::debug!(target: "atexit", "calling atexit from ctrlc handler");

        // Exit pager to restore terminal states (ex. quit raw mode)
        if let Ok(io) = clidispatch::io::IO::main() {
            let _ = io.quit_pager();

            // Attempt to reset terminal back to a good state. In particular, this fixes
            // the terminal if a Python `input()` call (with readline) is interrupted by
            // ctrl-c.
            let _ = io.reset_term();

            // Wait up to 5ms to clear out any progress output.
            let (send, recv) = channel::<()>();
            std::thread::spawn(move || {
                let _ = io.disable_progress(true);
                drop(send);
            });
            let _ = recv.recv_timeout(Duration::from_millis(5));
        }

        // Run atexit handlers.
        atexit::drop_queued();

        // "exit" tries to call "Drop"s but we don't rely on "Drop" for data integrity.
        std::process::exit(128 | libc::SIGINT);
    });
}

fn setup_nodeipc() {
    // Trigger `Lazy` initialization.
    let _ = nodeipc::get_singleton();
}

// Useful to prevent a commandserver connecting to another commandserver.
static IS_COMMANDSERVER: AtomicBool = AtomicBool::new(false);

fn commandserver_serve(args: &[String], io: &IO) -> i32 {
    IS_COMMANDSERVER.store(true, Ordering::Release);

    #[cfg(unix)]
    unsafe {
        libc::setsid();
    }

    let _ = setup_tracing_io(io, None);
    tracing::debug!("preparing commandserver");

    let python = HgPython::new(args);
    if let Err(e) = python.pre_import_modules() {
        tracing::warn!("cannot pre-import modules:\n{:?}", &e);
        return 1;
    }

    let run_func = |server: &Server, args: Vec<String>| -> i32 {
        tracing::debug!("commandserver is about to run command: {:?}", &args);
        if let Err(e) = python.setup_ui_system(server) {
            tracing::warn!("cannot setup ui.system:\n{:?}", &e);
        }
        run_command(args, io)
    };

    tracing::debug!("commandserver is about to serve");
    if let Err(e) = commandserver::server::serve_one_client(&run_func) {
        tracing::warn!("cannot serve:\n{:?}", &e);
        return 1;
    }
    tracing::debug!("commandserver is about to exit cleanly");
    0
}
