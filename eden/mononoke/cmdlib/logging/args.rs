/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::{ArgEnum, Args};

/// Command line arguments for spawning slog Logger
#[derive(Args, Debug)]
pub struct LoggingArgs {
    /// Print debug output
    #[clap(long)]
    pub debug: bool,

    /// Log level to use (does not work with --debug)
    #[clap(long, conflicts_with = "debug", possible_values = &slog::LOG_LEVEL_NAMES)]
    pub log_level: Option<String>,

    /// Include only log messages with these slog::Record::tags()/log::Record::targets
    #[clap(long, short = 'l')]
    pub log_include_tag: Vec<String>,

    // Exclude log messages with these slog::Record::tags()/log::Record::targets
    #[clap(long, short = 'L')]
    pub log_exclude_tag: Vec<String>,

    /// Logview category to log to. Logview is not used if not set
    #[clap(long)]
    pub logview_category: Option<String>,

    /// Logview level to filter. If logview category is not set then this is
    /// ignored. Note that this level is applied AFTER --log-level/--debug was
    /// applied, so it doesn't make sense to set this parameter to a lower level
    /// than --log-level
    #[clap(long, requires = "logview-category", possible_values = &slog::LOG_LEVEL_NAMES)]
    pub logview_additional_level_filter: Option<String>,

    /// Port for fb303 service
    #[clap(long, value_name = "PORT")]
    pub fb303_thrift_port: Option<u32>,

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

/// Command line arguments that control scuba logging
#[derive(Args, Debug)]
pub struct ScubaLoggingArgs {
    /// The name of the scuba dataset to log to
    #[clap(long)]
    pub scuba_dataset: Option<String>,
    /// A log file to write JSON Scuba logs to (primarily useful in testing)
    #[clap(long)]
    pub scuba_log_file: Option<String>,
    /// Do not use the default scuba dataset for this app
    #[clap(long)]
    pub no_default_scuba_dataset: bool,
    /// Special dataset to be used by warm bookmark cache.  If a binary doesn't
    /// use warm bookmark cache then this parameter is ignored
    #[clap(long)]
    pub warm_bookmark_cache_scuba_dataset: Option<String>,
}

#[derive(ArgEnum, Clone, Copy, Debug)]
#[clap(rename_all = "lower")]
pub enum PanicFate {
    None,
    Continue,
    Exit,
    Abort,
}
