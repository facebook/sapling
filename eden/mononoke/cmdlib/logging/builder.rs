/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;
use std::sync::Arc;

use anyhow::{format_err, Context, Error, Result};
use cached_config::ConfigStore;
use fbinit::FacebookInit;
use observability::{DynamicLevelDrain, ObservabilityContext};
use panichandler::{self, Fate};
use scuba_ext::MononokeScubaSampleBuilder;
use slog::{debug, o, Drain, Level, Logger, Never, SendSyncRefUnwindSafeDrain};
use slog_ext::make_tag_filter_drain;
use slog_glog_fmt::{kv_categorizer::FacebookCategorizer, kv_defaults::FacebookKV, GlogFormat};
use slog_term::TermDecorator;
use tunables::tunables;

use crate::args::{LoggingArgs, PanicFate, ScubaLoggingArgs};

pub fn create_log_level(logging_args: &LoggingArgs) -> Level {
    if logging_args.debug {
        Level::Debug
    } else {
        match &logging_args.log_level {
            Some(log_level_str) => Level::from_str(log_level_str)
                .unwrap_or_else(|_| panic!("Unknown log level: {}", log_level_str)),
            None => Level::Info,
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

// Logic copied from: https://fburl.com/code/ygj4muxz
pub fn create_root_log_drain(
    fb: FacebookInit,
    logging_args: &LoggingArgs,
    log_level: Level,
) -> Result<impl Drain<Ok = (), Err = Never> + Clone> {
    // Set the panic handler up here. Not really relevent to logger other than it emits output
    // when things go wrong. This writes directly to stderr as coredumper expects.
    // TODO: separate the panic handler out from logging
    let fate = match logging_args.panic_fate {
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
        logging_args.log_include_tag.iter().cloned().collect(),
        logging_args.log_include_tag.iter().cloned().collect(),
        true, // Log messages which have no tags
    )?;

    let root_log_drain: Arc<dyn SendSyncRefUnwindSafeDrain<Ok = (), Err = Never>> =
        match &logging_args.logview_category {
            Some(category) => {
                #[cfg(fbcode_build)]
                {
                    // Sometimes scribe writes can fail due to backpressure - it's OK to drop these
                    // since logview is sampled anyway.
                    let logview_drain =
                        ::slog_logview::LogViewDrain::new(fb, category).ignore_res();
                    match &logging_args.logview_additional_level_filter {
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

pub fn create_logger<T>(
    logging_args: &LoggingArgs,
    root_log_drain: T,
    observability_context: ObservabilityContext,
) -> Result<Logger>
where
    T: SendSyncRefUnwindSafeDrain<Ok = (), Err = Never> + Clone + std::panic::UnwindSafe + 'static,
{
    let root_log_drain = DynamicLevelDrain::new(root_log_drain, observability_context);

    let kv = FacebookKV::new().context("Failed to initialize FacebookKV")?;

    let logger = if logging_args.fb303_thrift_port.is_some() {
        Logger::root(slog_stats::StatsDrain::new(root_log_drain), o![kv])
    } else {
        Logger::root(root_log_drain, o![kv])
    };

    Ok(logger)
}

pub fn create_observability_context(
    logging_args: &LoggingArgs,
    config_store: &ConfigStore,
    log_level: slog::Level,
) -> Result<ObservabilityContext> {
    if logging_args.with_dynamic_observability {
        Ok(ObservabilityContext::new(config_store)?)
    } else {
        Ok(ObservabilityContext::new_static(log_level))
    }
}

pub fn create_scuba_sample_builder(
    fb: FacebookInit,
    scuba_args: &ScubaLoggingArgs,
    observability_context: &ObservabilityContext,
) -> Result<MononokeScubaSampleBuilder> {
    let default_scuba_set = None; // TODO get default scuba set from somewhere
    let mut scuba_logger = if let Some(scuba_dataset) = &scuba_args.scuba_dataset {
        MononokeScubaSampleBuilder::new(fb, scuba_dataset.as_str())
    } else if let Some(default_scuba_dataset) = default_scuba_set {
        if scuba_args.no_default_scuba_dataset {
            MononokeScubaSampleBuilder::with_discard()
        } else {
            MononokeScubaSampleBuilder::new(fb, default_scuba_dataset)
        }
    } else {
        MononokeScubaSampleBuilder::with_discard()
    };
    if let Some(scuba_log_file) = &scuba_args.scuba_log_file {
        scuba_logger = scuba_logger.with_log_file(scuba_log_file.clone())?;
    }
    let mut scuba_logger = scuba_logger
        .with_observability_context(observability_context.clone())
        .with_seq("seq");

    scuba_logger.add_common_server_data();

    Ok(scuba_logger)
}

pub fn create_warm_bookmark_cache_scuba_sample_builder(
    fb: FacebookInit,
    scuba_args: &ScubaLoggingArgs,
) -> Result<MononokeScubaSampleBuilder, Error> {
    let maybe_scuba = match scuba_args.warm_bookmark_cache_scuba_dataset.clone() {
        Some(scuba) => {
            let tw_task_id =
                std::env::var("TW_TASK_ID").context("failed to get TW_TASK_ID env var")?;
            let tw_task_id: u32 = tw_task_id
                .parse()
                .context("failed to parse TW_TASK_ID env var")?;
            let mut sampling = tunables().get_warm_bookmark_cache_loggin_tw_task_sampling() as u32;
            if sampling == 0 {
                sampling = 10;
            }

            if tw_task_id % sampling == 0 {
                Some(scuba)
            } else {
                None
            }
        }
        None => None,
    };

    Ok(MononokeScubaSampleBuilder::with_opt_table(fb, maybe_scuba))
}
