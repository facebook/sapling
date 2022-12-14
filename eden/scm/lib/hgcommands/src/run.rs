/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::fs::File;
use std::io;
use std::io::BufWriter;
use std::io::Write;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;
use std::sync::Weak;
use std::thread;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use anyhow::Result;
use blackbox::serde_json;
use clidispatch::dispatch;
use clidispatch::dispatch::Dispatcher;
use clidispatch::errors;
use clidispatch::global_flags::HgGlobalOpts;
use clidispatch::io::IsTty;
use clidispatch::io::IO;
use configloader::config::ConfigSet;
use configmodel::Config;
use configmodel::ConfigExt;
use fail::FailScenario;
use once_cell::sync::Lazy;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use progress_model::Registry;
use repo::repo::Repo;
use tracing::dispatcher;
use tracing::dispatcher::Dispatch;
use tracing::Level;
use tracing_collector::TracingData;
use tracing_sampler::SamplingConfig;
use tracing_sampler::SamplingLayer;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::Layer as FmtLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

use crate::commands;
use crate::HgPython;

/// Run a Rust or Python command.
///
/// Have side effect on `io` and return the command exit code.
pub fn run_command(args: Vec<String>, io: &IO) -> i32 {
    let now = SystemTime::now();

    // The pfcserver does not want tracing or blackbox setup, or going through
    // the Rust command table. Bypass them.
    if args.get(1).map(|s| s.as_ref()) == Some("start-pfc-server") {
        return HgPython::new(&args).run_hg(args, io, &ConfigSet::new());
    }
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
        return hgpython.run_python(&args, io) as i32;
    }

    // Extra initialization based on global flags.
    let global_opts = dispatch::parse_global_opts(&args[1..]).ok();

    // This allows us to defer the SamplingLayer initialization until after the repo config is loaded.
    let sampling_config = Arc::new(OnceCell::<SamplingConfig>::new());

    // Setup tracing early since "log_start" will use it immediately.
    // The tracing clock starts ticking from here.
    let tracing_data = match setup_tracing(&global_opts, io, sampling_config.clone()) {
        Err(_) => {
            // With our current architecture it is common to see this path in our tests due to
            // trying to set a global collector a second time. Ignore the error and return some
            // dummy values. FIXME!
            Arc::new(Mutex::new(TracingData::new()))
        }
        Ok(res) => res,
    };

    let scenario = setup_fail_points();
    setup_eager_repo();

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
    let span = log_start(args.clone(), now);

    // Ad-hoc environment variable: EDENSCM_TRACE_OUTPUT. A more standard way
    // to access the data is via the blackbox interface.
    let trace_output_path = std::env::var("EDENSCM_TRACE_OUTPUT").ok();
    if trace_output_path.is_some() {
        // Unset environment variable so processes forked by this command
        // wouldn't rewrite the trace.
        std::env::remove_var("EDENSCM_TRACE_OUTPUT");
    }

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

                if let Some(sc) = SamplingConfig::new(dispatcher.config()) {
                    sampling_config.set(sc).unwrap();
                }

                dispatch_command(io, dispatcher, cwd, Arc::downgrade(&in_scope), now)
            }
            Err(err) => {
                errors::print_error(&err, io, &args[1..]);
                255
            }
        }
    })();

    span.record("exit_code", &exit_code);
    drop(in_scope);

    let _ = maybe_write_trace(io, &tracing_data, trace_output_path);

    log_end(exit_code as u8, now, tracing_data, &span);

    // Sync the blackbox before returning: this exit code is going to be used to process::exit(),
    // so we need to flush now.
    blackbox::sync();

    if let Some(scenario) = scenario {
        scenario.teardown();
        FAIL_SETUP.store(false, SeqCst);
    }

    exit_code
}

fn dispatch_command(
    io: &IO,
    mut dispatcher: Dispatcher,
    cwd: PathBuf,
    in_scope: Weak<()>,
    start_time: SystemTime,
) -> i32 {
    log_repo_path_and_exe_version(dispatcher.repo());

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
        Err(err) => {
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
                fell_back = true;
                // Change the current dir back to the original so it is not surprising to the Python
                // code.
                let _ = env::set_current_dir(cwd);

                let mut interp = HgPython::new(dispatcher.args());
                if dispatcher.global_opts().trace {
                    // Error is not fatal.
                    let _ = interp.setup_tracing("*".into());
                }
                interp.run_hg(dispatcher.args().to_vec(), io, config)
            } else {
                errors::print_error(&err, io, &dispatcher.args()[1..]);
                255
            }
        }
    };

    if !fell_back {
        if let Some(command) = command {
            let hooks_with_prefix =
                |prefix: String| -> Vec<minibytes::Text> { config.keys_prefixed("hooks", &prefix) };
            let mut hooks = hooks_with_prefix(format!("pre-{}.", command.main_alias()));
            if exit_code > 0 {
                let mut names = hooks_with_prefix(format!("fail-{}.", command.main_alias()));
                hooks.append(&mut names);
            } else {
                let mut names = hooks_with_prefix(format!("post-{}.", command.main_alias()));
                hooks.append(&mut names);
            }

            if !hooks.is_empty() {
                let _ = io.write_err(format!(
                    "WARNING: The following hooks were not run: {:?}\n",
                    hooks
                ));
            }
        }
        if io.wait_pager().is_err() {
            return 255;
        }
    }

    // Clean up progress models.
    Registry::main().remove_orphan_models();

    if let Some(rl) = &run_logger {
        if let Err(err) = rl.close(exit_code) {
            // Command has already finished - not worth bailing due to this error.
            let _ = io.write_err(format!("Error writing final runlog: {}\n", err));
        }
    }

    let _ = log_perftrace(io, config, start_time);

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

fn setup_tracing(
    global_opts: &Option<HgGlobalOpts>,
    io: &IO,
    sampling_config: Arc<OnceCell<SamplingConfig>>,
) -> Result<Arc<Mutex<TracingData>>> {
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

    let is_test = is_inside_test();
    let mut env_filter_dirs: Option<String> = ["EDENSCM_LOG", "LOG"]
        .iter()
        .take(if is_test { 2 } else { 1 }) /* Only consider $LOG in tests */
        .filter_map(|s| std::env::var(s).ok())
        .next();
    // Ensure EnvFilter is used in tests so it can be changed on the
    // fly. Don't enable if EDENSCM_TRACE_LEVEL is set because that
    // indicates test is testing tracing/sampling.
    if is_test && std::env::var("EDENSCM_TRACE_LEVEL").is_err() && env_filter_dirs.is_none() {
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

        // The env_filter does the actual filtering. No need to filter by level.
        let collector = tracing_collector::default_collector(data.clone(), Level::TRACE);
        let env_logger = FmtLayer::new()
            .with_span_events(FmtSpan::ACTIVE)
            .with_ansi(can_color)
            .with_writer(tracing_reload::reloadable_writer);
        if is_test {
            // In tests, disable color and timestamps for cleaner output.
            let env_logger = env_logger.without_time().with_ansi(false);
            let collector = collector.with(env_filter.and_then(env_logger));
            tracing::subscriber::set_global_default(collector)?;
        } else {
            let collector = collector.with(env_filter.and_then(env_logger));
            tracing::subscriber::set_global_default(collector)?;
        }
    } else {
        let level = std::env::var("EDENSCM_TRACE_LEVEL")
            .ok()
            .and_then(|s| Level::from_str(&s).ok())
            .unwrap_or_else(|| {
                if let Some(opts) = global_opts {
                    if opts.trace {
                        return Level::DEBUG;
                    }
                }
                Level::INFO
            });

        let collector = tracing_collector::default_collector(data.clone(), level)
            .with(SamplingLayer::new(sampling_config));
        tracing::subscriber::set_global_default(collector)?;
    }

    Ok(data)
}

fn spawn_progress_thread(
    config: &ConfigSet,
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

    let assume_tty = config.get_or("progress", "assume-tty", || false)?;
    if !assume_tty && !io.error().is_tty() {
        disable_rendering = true;
    }

    if global_opts.quiet || global_opts.debug || hgplain::is_plain(Some("progress")) {
        disable_rendering = true;
    }

    let render_function = progress_render::simple::render;
    let renderer_name = config.get_or("progress", "renderer", || "rust:simple".to_string())?;
    if renderer_name == "none" {
        disable_rendering = true;
    }

    let interval = Duration::from_secs_f64(config.get_or("progress", "refresh", || 0.1)?)
        .max(Duration::from_millis(50));

    // lockstep is used by tests to control progress rendering run loop.
    let lockstep = config.get_or("progress", "lockstep", || false)?;

    // Limit how often we write runlog. This config knob is primarily for tests to lower.
    let runlog_interval =
        Duration::from_secs_f64(config.get_or("runlog", "progress-refresh", || 0.5)?).max(interval);

    let progress = io.progress();

    let mut config = progress_render::RenderingConfig {
        delay: Duration::from_secs_f64(config.get_or("progress", "delay", || 3.0)?),
        term_width: progress.term_size().0,
        ..Default::default()
    };

    let registry = Registry::main();

    let mut stderr = io.error();

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

            if !disable_rendering {
                config.term_width = progress.term_size().0;

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
                        let _ = write!(stderr, "Error updating runlog progress: {}\n", err);
                    }

                    last_runlog_time = Some(now);
                }
            }

            registry.remove_orphan_progress_bar();

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
        Box::new(io.error())
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

fn log_start(args: Vec<String>, now: SystemTime) -> tracing::Span {
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

    if let Ok(tags) = std::env::var("EDENSCM_BLACKBOX_TAGS") {
        tracing::info!(name = "blackbox_tags", tags = AsRef::<str>::as_ref(&tags));
        let names: Vec<String> = tags.split_whitespace().map(ToString::to_string).collect();
        blackbox::log(&blackbox::event::Event::Tags { names });
    }

    let mut parent_names = Vec::new();
    let mut parent_pids = Vec::new();
    if !inside_test {
        let mut ppid = procinfo::parent_pid(0);
        // In theory, the OS should not report a cyclic process graph (ex. pid 1
        // has parent pid = 1). Practically `parent_pids` takes snapshots
        // everytime on Windows (unnecessarily) and is subject to races. Be
        // extra careful here so the loop wouldn't be infinite.
        while ppid != 0 && parent_pids.len() < 16 && !parent_pids.contains(&ppid) {
            let name = procinfo::exe_name(ppid);
            parent_names.push(name);
            parent_pids.push(ppid);
            ppid = procinfo::parent_pid(ppid);
        }
    }

    let span = tracing::info_span!(
        "Run Command",
        pid = pid,
        uid = uid,
        nice = nice,
        args = AsRef::<str>::as_ref(&serde_json::to_string(&args).unwrap()),
        parent_pids = AsRef::<str>::as_ref(&serde_json::to_string(&parent_pids).unwrap()),
        parent_names = AsRef::<str>::as_ref(&serde_json::to_string(&parent_names).unwrap()),
        version = version::VERSION,
        // Reserved for log_end.
        exit_code = 0,
        max_rss = 0,
    );

    blackbox::log(&blackbox::event::Event::Start {
        pid,
        uid,
        nice,
        args,
        timestamp_ms: epoch_ms(now),
    });

    blackbox::log(&blackbox::event::Event::ProcessTree {
        names: parent_names,
        pids: parent_pids,
    });

    span
}

fn log_end(
    exit_code: u8,
    now: SystemTime,
    tracing_data: Arc<Mutex<TracingData>>,
    span: &tracing::Span,
) {
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

    span.record("exit_code", &exit_code);
    span.record("max_rss", &max_rss);

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

    // Truncate duration to top three significant decimal digits of
    // precision to reduce cardinality for logging storage.
    tracing::debug!(target: "measuredtimes", command_duration=util::math::truncate_int(duration_ms, 3));
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

fn log_repo_path_and_exe_version(repo: Option<&Repo>) {
    // The "version" and "repo" fields are consumed by telemetry.
    if let Some(repo) = repo {
        let config = repo.config();
        let opt_path_default: std::result::Result<Option<String>, _> =
            config.get_or_default("paths", "default");
        if let Ok(Some(path_default)) = opt_path_default {
            if let Some(repo_name) = configloader::hg::repo_name_from_url(&path_default) {
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

fn log_perftrace(io: &IO, config: &ConfigSet, start_time: SystemTime) -> Result<()> {
    if let Some(threshold) = config.get_opt::<Duration>("tracing", "threshold")? {
        if let Ok(elapsed) = start_time.elapsed() {
            if elapsed >= threshold {
                let key = format!(
                    "flat/perftrace-{}-{}-{}",
                    hostname::get()?.to_string_lossy(),
                    std::process::id(),
                    (epoch_ms(start_time) as f64) / 1e3,
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
    }

    Ok(())
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

fn setup_eager_repo() {
    static REGISTERED: Lazy<()> = Lazy::new(|| {
        edenapi::Builder::register_customize_build_func(eagerepo::edenapi_from_config)
    });

    *REGISTERED
}

static FAIL_SETUP: AtomicBool = AtomicBool::new(false);

fn setup_fail_points<'a>() -> Option<FailScenario<'a>> {
    if std::env::var("FAILPOINTS").is_err() {
        // No need to setup failpoints.
        return None;
    }
    if FAIL_SETUP.fetch_or(true, SeqCst) {
        // Already setup.
        None
    } else {
        Some(FailScenario::setup())
    }
}
