/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod commands;
mod run;

pub use cmdpy::HgPython;
pub use cmdpy::prepare_builtin_modules;
pub use run::run_command;

/// Register Rust functions required by `cmdpy`. Can be called multiple times.
/// This is used by `run_command` and `pybindings`.
///
/// Register the Python hook runner.
pub fn init() {
    use cmdpy::RustCommandConfig;
    let cfg = RustCommandConfig {
        table: commands::table,
        run_command,
    };
    cfg.register();

    // Enables lib/hook to run Python hooks.
    cmdpy::init();
}

pub fn deinit() {
    cmdpy::deinit();
}
