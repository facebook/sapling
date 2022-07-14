/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(fbcode_build)]
use anyhow::format_err;
use anyhow::Context;
use anyhow::Result;
use cached_config::ConfigStore;
use clap::ArgEnum;
use clap::Args;
use fbinit::FacebookInit;
use observability::ObservabilityContext;
use panichandler::Fate;
use slog::debug;
use slog::o;
use slog::Drain;
use slog::Level;
use slog::Logger;
use slog::Never;
use slog::SendSyncRefUnwindSafeDrain;
use slog_ext::make_tag_filter_drain;
use slog_glog_fmt::kv_categorizer::FacebookCategorizer;
use slog_glog_fmt::kv_defaults::FacebookKV;
use slog_glog_fmt::GlogFormat;
use slog_term::TermDecorator;
use std::str::FromStr;
use std::sync::Arc;

/// Command line arguments for spawning slog Logger
#[derive(Args, Debug)]
pub struct LoggingArgs {
    /// Print debug output
    ///
    /// Equivalent to --log-level=DEBUG.
    #[clap(long)]
    pub debug: bool,

    /// Log level to use
    #[clap(long, conflicts_with = "debug", possible_values = &slog::LOG_LEVEL_NAMES)]
    pub log_level: Option<String>,

    /// Include only log messages with these slog::Record::tags() or
    /// log::Record::targets
    #[clap(long, short = 'l')]
    pub log_include_tag: Vec<String>,

    /// Exclude log messages with these slog::Record::tags() or
    /// log::Record::targets
    #[clap(long, short = 'L')]
    pub log_exclude_tag: Vec<String>,

    /// Logview category to log to. Logview is not used if not set
    #[clap(long)]
    pub logview_category: Option<String>,

    /// Logview level to filter
    ///
    /// If logview-category is not set then this is ignored.
    ///
    /// Note that this level is applied AFTER --log-level/--debug was applied,
    /// so it doesn't make sense to set this parameter to a lower level than
    /// --log-level.
    #[clap(long, requires = "logview-category", possible_values = &slog::LOG_LEVEL_NAMES)]
    pub logview_additional_level_filter: Option<String>,

    /// Fate of the process when a panic happens
    #[clap(long, arg_enum, default_value_t=PanicFate::Abort)]
    pub panic_fate: PanicFate,

    /// Whether to instantiate ObservabilityContext::Dynamic, which reads
    /// logging levels from configerator. Overwrites --log-level or --debug
    // For compatibility with existing usage, this arg takes value,
    // for example `--with-dynamic-observability=true`.
    #[clap(
        long,
        parse(try_from_str),
        default_value_t = false,
        value_name = "BOOL"
    )]
    pub with_dynamic_observability: bool,
}

#[derive(ArgEnum, Clone, Copy, Debug)]
#[clap(rename_all = "lower")]
pub enum PanicFate {
    None,
    Continue,
    Exit,
    Abort,
}

impl LoggingArgs {
    pub fn create_log_level(&self) -> Level {
        if self.debug {
            Level::Debug
        } else {
            match &self.log_level {
                Some(log_level_str) => Level::from_str(log_level_str)
                    .unwrap_or_else(|_| panic!("Unknown log level: {}", log_level_str)),
                None => Level::Info,
            }
        }
    }

    // Logic copied from: https://fburl.com/code/ygj4muxz
    pub fn create_root_log_drain(
        &self,
        fb: FacebookInit,
        log_level: Level,
    ) -> Result<impl Drain<Ok = (), Err = Never> + Clone> {
        // Set the panic handler up here. Not really relevent to logger other than it emits output
        // when things go wrong. This writes directly to stderr as coredumper expects.
        // TODO: separate the panic handler out from logging
        let fate = match self.panic_fate {
            PanicFate::None => None,
            PanicFate::Continue => Some(Fate::Continue),
            PanicFate::Exit => Some(Fate::Exit(101)),
            PanicFate::Abort => Some(Fate::Abort),
        };
        if let Some(fate) = fate {
            panichandler::set_panichandler(fate);
        }

        let stdlog_env = "RUST_LOG";

        let glog_drain = make_tag_filter_drain(
            glog_drain(),
            self.log_include_tag.iter().cloned().collect(),
            self.log_exclude_tag.iter().cloned().collect(),
            true, // Log messages which have no tags
        )?;

        let root_log_drain: Arc<dyn SendSyncRefUnwindSafeDrain<Ok = (), Err = Never>> = match &self
            .logview_category
        {
            Some(category) => {
                #[cfg(fbcode_build)]
                {
                    // Sometimes scribe writes can fail due to backpressure - it's OK to drop these
                    // since logview is sampled anyway.
                    let logview_drain =
                        ::slog_logview::LogViewDrain::new(fb, category).ignore_res();
                    match &self.logview_additional_level_filter {
                        Some(log_level_str) => {
                            let logview_level = Level::from_str(log_level_str)
                                .map_err(|_| format_err!("Unknown log level: {}", log_level_str))?;

                            let drain = slog::Duplicate::new(
                                glog_drain,
                                logview_drain.filter_level(logview_level).ignore_res(),
                            );
                            Arc::new(drain.ignore_res())
                        }
                        None => {
                            let drain = slog::Duplicate::new(glog_drain, logview_drain);
                            Arc::new(drain.ignore_res())
                        }
                    }
                }
                #[cfg(not(fbcode_build))]
                {
                    let _ = (fb, category);
                    unimplemented!(
                        "Passed --logview-category, but it is supported only for fbcode builds",
                    )
                }
            }
            None => Arc::new(glog_drain),
        };

        // NOTE: We pass an unfiltered Logger to init_stdlog_once. That's because we do the filtering
        // at the stdlog level there.
        let stdlog_logger = Logger::root(root_log_drain.clone(), o![]);
        let stdlog_level = crate::log::init_stdlog_once(stdlog_logger, stdlog_env)?;

        // Note what level we enabled stdlog at, so that if someone is trying to debug they get
        // informed of potentially needing to set RUST_LOG.
        debug!(
            Logger::root(
                root_log_drain.clone().filter_level(log_level).ignore_res(),
                o![]
            ),
            "enabled stdlog with level: {:?} (set {} to configure)", stdlog_level, stdlog_env
        );

        Ok(root_log_drain)
    }

    pub fn create_logger(
        &self,
        root_log_drain: Arc<dyn SendSyncRefUnwindSafeDrain<Ok = (), Err = Never>>,
    ) -> Result<Logger> {
        let kv = FacebookKV::new().context("Failed to initialize FacebookKV")?;
        Ok(Logger::root(root_log_drain, o![kv]))
    }

    pub fn create_observability_context(
        &self,
        config_store: &ConfigStore,
        log_level: slog::Level,
    ) -> Result<ObservabilityContext> {
        if self.with_dynamic_observability {
            Ok(ObservabilityContext::new(config_store)?)
        } else {
            Ok(ObservabilityContext::new_static(log_level))
        }
    }
}

/// Create a default root logger for Facebook services
fn glog_drain() -> impl Drain<Ok = (), Err = Never> {
    let decorator = TermDecorator::new().build();
    // FacebookCategorizer is used for slog KV arguments.
    // At the time of writing this code FacebookCategorizer and FacebookKV
    // that was added below was mainly useful for logview logging and had no effect on GlogFormat
    let drain = GlogFormat::new(decorator, FacebookCategorizer).ignore_res();
    ::std::sync::Mutex::new(drain).ignore_res()
}
