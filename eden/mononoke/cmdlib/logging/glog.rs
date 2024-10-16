/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use fbinit::FacebookInit;
use slog::Level;

/// Sets the log level used by glog library to match the level use we use in slog.
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
            }
        }
        _ => {
            let level_glog_num = match level {
                Level::Critical => 4,
                Level::Error => 3,
                Level::Warning => 2,
                // Normally glog 1 corresponds to our INFO but since we normally
                // care very little about things logged by our deps it might be
                // easier to delegate those to DEBUG output.
                Level::Info => 2,
                _ => 0,
            };
            gflags::set_gflag_value(fb, "minloglevel", gflags::GflagValue::U32(level_glog_num))?;
        }
    }
    Ok(())
}
