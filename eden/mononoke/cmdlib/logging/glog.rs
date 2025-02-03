/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use fbinit::FacebookInit;
use slog::Level;

/// Sets the log level used by the glog and C++ logging libraries to match the level use we use in slog.
/// It can be overridden by setting the GLOG_minloglevel env variable.
pub fn set_glog_log_level(fb: FacebookInit, level: Level) -> Result<()> {
    match std::env::var("GLOG_minloglevel") {
        Ok(level) => {
            if let Ok(level_glog_num) = level.parse::<u32>() {
                // set the flag again, some libraries we use might have overriden it.
                gflags::set_gflag_value(
                    fb,
                    "minloglevel",
                    gflags::GflagValue::U32(level_glog_num),
                )?;

                // VERBOSE=0, INFO=1, WARNING=2, ERROR=3, CRITICAL=4, FATAL=5, SHUTDOWN=6
                // https://www.internalfb.com/code/fbsource/[dd6eb36b8d01]/third-party/tp2/glog/0.3.2_fb/glog-0.3.2/src/glog/log_severity.h?lines=51-52
                let logging_level = match level_glog_num {
                    6 => "FATAL",
                    5 => "FATAL",
                    4 => "CRITICAL",
                    3 => "ERR",
                    2 => "WARN",
                    _ => "INFO", // Ignore debug and lower levels
                };
                logging::update_logging_config(fb, logging_level);
            }
        }
        _ => {
            let (level_glog_num, logging_level) = match level {
                Level::Critical => (4, "CRITICAL"),
                Level::Error => (3, "ERR"),
                Level::Warning => (2, "WARN"),
                // Normally glog 1 corresponds to our INFO but since we normally
                // care very little about things logged by our deps it might be
                // easier to delegate those to DEBUG output.
                Level::Info => (2, "WARN"),
                _ => (0, "INFO"),
            };
            gflags::set_gflag_value(fb, "minloglevel", gflags::GflagValue::U32(level_glog_num))?;
            logging::update_logging_config(fb, logging_level);
        }
    }
    Ok(())
}
