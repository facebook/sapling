/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use configloader::hg::PinnedConfig;

use crate::global_flags::HgGlobalOpts;

#[macro_export]
macro_rules! abort_if {
    ( $cond:expr, $($arg:tt)+ ) => {
        if $cond {
            $crate::abort!($($arg)*);
        }
    };
}

#[macro_export]
macro_rules! abort {
    ( $($arg:tt)+ ) => {
        return Err($crate::errors::Abort(format!($($arg)*).into()).into());
    };
}

#[macro_export]
macro_rules! fallback {
    ( $($arg:tt)+ ) => {
        return Err($crate::errors::FallbackToPython(format!($($arg)*).into()).into());
    };
}

pub(crate) fn pinned_configs(global_opts: &HgGlobalOpts) -> Vec<PinnedConfig> {
    let mut pinned = PinnedConfig::from_cli_opts(&global_opts.config, &global_opts.configfile);

    if global_opts.hidden {
        pinned.push(PinnedConfig::KeyValue(
            "visibility".into(),
            "all-heads".into(),
            "true".into(),
            "--hidden".into(),
        ));
    }

    if global_opts.trace || global_opts.traceback {
        pinned.push(PinnedConfig::KeyValue(
            "ui".into(),
            "traceback".into(),
            "on".into(),
            "--traceback".into(),
        ));
    }
    if global_opts.profile {
        pinned.push(PinnedConfig::KeyValue(
            "profiling".into(),
            "enabled".into(),
            "true".into(),
            "--profile".into(),
        ));
    }
    if !global_opts.color.is_empty() {
        pinned.push(PinnedConfig::KeyValue(
            "ui".into(),
            "color".into(),
            global_opts.color.clone().into(),
            "--color".into(),
        ));
    }
    if global_opts.verbose || global_opts.debug || global_opts.quiet {
        pinned.push(PinnedConfig::KeyValue(
            "ui".into(),
            "verbose".into(),
            global_opts.verbose.to_string().into(),
            "--verbose".into(),
        ));
        pinned.push(PinnedConfig::KeyValue(
            "ui".into(),
            "debug".into(),
            global_opts.debug.to_string().into(),
            "--debug".into(),
        ));
        pinned.push(PinnedConfig::KeyValue(
            "ui".into(),
            "quiet".into(),
            global_opts.quiet.to_string().into(),
            "--quiet".into(),
        ));
    }
    if global_opts.noninteractive {
        pinned.push(PinnedConfig::KeyValue(
            "ui".into(),
            "interactive".into(),
            "off".into(),
            "-y".into(),
        ));
    }

    pinned
}

#[cfg(test)]
mod test {
    fn abort_if_simple(should_abort: bool) -> anyhow::Result<()> {
        abort_if!(should_abort, "error!");
        Ok(())
    }

    fn abort_if_format(should_abort: bool) -> anyhow::Result<()> {
        abort_if!(should_abort, "error: {}", "banana");
        Ok(())
    }

    #[test]
    fn test_abort_if() {
        assert!(abort_if_simple(false).is_ok());
        assert_eq!(format!("{}", abort_if_simple(true).unwrap_err()), "error!");

        assert!(abort_if_format(false).is_ok());
        assert_eq!(
            format!("{}", abort_if_format(true).unwrap_err()),
            "error: banana",
        );
    }

    fn abort_simple() -> anyhow::Result<()> {
        abort!("error!");
    }

    fn abort_format() -> anyhow::Result<()> {
        abort!("error: {}", "banana");
    }

    fn abort_format_single() -> anyhow::Result<()> {
        let banana = "banana";
        abort!("error: {banana}");
    }

    #[test]
    fn test_abort() {
        assert_eq!(format!("{}", abort_simple().unwrap_err()), "error!");
        assert_eq!(format!("{}", abort_format().unwrap_err()), "error: banana",);
        assert_eq!(
            format!("{}", abort_format_single().unwrap_err()),
            "error: banana",
        );
    }
}
