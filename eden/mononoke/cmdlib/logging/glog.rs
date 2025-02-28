/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use fbinit::FacebookInit;

#[derive(Copy, Clone, Debug)]
pub enum GlogLevel {
    Shutdown,
    Fatal,
    Critical,
    Error,
    Warning,
    Info,
    Verbose,
}

impl GlogLevel {
    fn from_num(num: u32) -> GlogLevel {
        // VERBOSE=0, INFO=1, WARNING=2, ERROR=3, CRITICAL=4, FATAL=5, SHUTDOWN=6
        // https://www.internalfb.com/code/fbsource/[dd6eb36b8d01]/third-party/tp2/glog/0.3.2_fb/glog-0.3.2/src/glog/log_severity.h?lines=51-52
        match num {
            6 => GlogLevel::Shutdown,
            5 => GlogLevel::Fatal,
            4 => GlogLevel::Critical,
            3 => GlogLevel::Error,
            2 => GlogLevel::Warning,
            1 => GlogLevel::Info,
            _ => GlogLevel::Verbose,
        }
    }

    fn to_num(&self) -> u32 {
        match self {
            GlogLevel::Shutdown => 6,
            GlogLevel::Fatal => 5,
            GlogLevel::Critical => 4,
            GlogLevel::Error => 3,
            GlogLevel::Warning => 2,
            GlogLevel::Info => 1,
            GlogLevel::Verbose => 0,
        }
    }

    fn to_cpp_logging_level(&self) -> &'static str {
        match self {
            GlogLevel::Shutdown => "FATAL",
            GlogLevel::Fatal => "FATAL",
            GlogLevel::Critical => "CRITICAL",
            GlogLevel::Error => "ERR",
            GlogLevel::Warning => "WARN",
            GlogLevel::Info => "INFO",
            GlogLevel::Verbose => "INFO", // Ignore verbose level.
        }
    }
}

impl From<slog::Level> for GlogLevel {
    fn from(log_level: slog::Level) -> GlogLevel {
        use slog::Level;
        match log_level {
            Level::Critical => GlogLevel::Critical,
            Level::Error => GlogLevel::Error,
            Level::Warning => GlogLevel::Warning,
            // Reduce log spew in dependencies by limiting to warning at info level
            Level::Info => GlogLevel::Warning,
            // Reduce log spew in dependencies by limiting to info at debug and trace level
            Level::Debug | Level::Trace => GlogLevel::Info,
        }
    }
}

impl From<tracing_subscriber::filter::LevelFilter> for GlogLevel {
    fn from(log_level: tracing_subscriber::filter::LevelFilter) -> GlogLevel {
        use tracing_subscriber::filter::LevelFilter;
        match log_level {
            LevelFilter::OFF => GlogLevel::Fatal,
            LevelFilter::ERROR => GlogLevel::Error,
            LevelFilter::WARN => GlogLevel::Warning,
            // Reduce log spew in dependencies by limiting to warning at info level
            LevelFilter::INFO => GlogLevel::Warning,
            // Reduce log spew in dependencies by limiting to info at debug and trace level
            LevelFilter::DEBUG | LevelFilter::TRACE => GlogLevel::Info,
        }
    }
}

/// Sets the log level used by the glog and C++ logging libraries to match the level use we use.
/// It can be overridden by setting the GLOG_minloglevel env variable.
pub fn set_glog_log_level(fb: FacebookInit, level: GlogLevel) -> Result<()> {
    match std::env::var("GLOG_minloglevel") {
        Ok(level) => {
            if let Ok(level_glog_num) = level.parse::<u32>() {
                // set the flag again, some libraries we use might have overridden it.
                gflags::set_gflag_value(
                    fb,
                    "minloglevel",
                    gflags::GflagValue::U32(level_glog_num),
                )?;

                let logging_level = GlogLevel::from_num(level_glog_num).to_cpp_logging_level();
                logging::update_logging_config(fb, logging_level);
            }
        }
        _ => {
            let level_glog_num = level.to_num();
            let logging_level = level.to_cpp_logging_level();
            gflags::set_gflag_value(fb, "minloglevel", gflags::GflagValue::U32(level_glog_num))?;
            logging::update_logging_config(fb, logging_level);
        }
    }
    Ok(())
}
