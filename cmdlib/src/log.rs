// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use env_logger::filter::{Builder, Filter};
use log;
use slog::{BorrowedKV, Level, Logger, SingleKV};

// NOTE: The following 2 methods and parts of the implementation of LinkedLogger::log are borrowed
// from the slog-stdlog crate (MIT-licensed).

fn log_to_slog_level(level: log::Level) -> Level {
    match level {
        log::Level::Trace => Level::Trace,
        log::Level::Debug => Level::Debug,
        log::Level::Info => Level::Info,
        log::Level::Warn => Level::Warning,
        log::Level::Error => Level::Error,
    }
}

fn record_as_location(r: &log::Record) -> slog::RecordLocation {
    let module = r.module_path_static().unwrap_or("<unknown>");
    let file = r.file_static().unwrap_or("<unknown>");
    let line = r.line().unwrap_or_default();

    slog::RecordLocation {
        file,
        line,
        column: 0,
        function: "",
        module,
    }
}

struct LinkedLogger {
    logger: Logger,
    filter: Filter,
}

impl log::Log for LinkedLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        self.filter.enabled(m)
    }

    fn log(&self, r: &log::Record) {
        if !self.filter.matches(r) {
            return;
        }

        let level = log_to_slog_level(r.metadata().level());

        let args = r.args();
        let target = r.target();
        let location = &record_as_location(r);
        let s = slog::RecordStatic {
            location,
            level,
            tag: target,
        };

        // NOTE: Normally, we'd want to use the recommended b! macro from Slog here... but it turns
        // out that expand this macro gives us something that looks like ::slog::BorrowedKV(&FOO);,
        // which means FOO gets dropped immediately and we get an error telling us to assign it to
        // a variable (FOO is the SingleKV here). This does that.
        let k = SingleKV::from(("target", target));
        let x = BorrowedKV(&k);

        let record = slog::Record::new(&s, args, x);

        self.logger.log(&record);
    }

    fn flush(&self) {}
}

/// Wire up a slog Logger as the destination for std logs as per an env_logger filter spec. This
/// sets the global logger, so it'll panic if called more than once.
pub fn init_stdlog_once(logger: Logger, var: &str) -> log::LevelFilter {
    // NOTE: The default level is ERROR, which should be fairly reasonable.
    let filter = Builder::from_env(var).build();
    let level = filter.filter();

    log::set_boxed_logger(Box::new(LinkedLogger { logger, filter })).unwrap();

    // set_max_level ensures we don't produce logs that won't pass any filter at all.
    log::set_max_level(level);

    level
}
