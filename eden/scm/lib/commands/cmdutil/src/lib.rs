/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Dependencies for command implementation. Shared code between `commands` and actual commands.

pub use anyhow::Error;
pub use anyhow::Result;
pub use clidispatch;
use clidispatch::errors::FallbackToPython;
use clidispatch::global_flags::HgGlobalOpts;
use clidispatch::io::Write;
pub use clidispatch::io::IO;
pub use clidispatch::ReqCtx;
pub use cliparser::define_flags;
pub use configmodel::Config;
pub use configmodel::ConfigExt;
pub use configset::config::ConfigSet;
pub use formatter::formatter;
pub use repo::Repo;

pub fn get_formatter(
    config: &dyn Config,
    command_name: &'static str,
    template: &str,
    options: &HgGlobalOpts,
    mut writer: Box<dyn Write>,
) -> Result<Box<dyn formatter::ListFormatter>, FallbackToPython> {
    formatter::get_formatter(
        config,
        command_name,
        template,
        formatter::FormatOptions {
            debug: options.debug,
            verbose: options.verbose,
            quiet: options.quiet,
            color: termstyle::should_color(config, writer.as_mut()),
            debug_color: config.get("ui", "color") == Some("debug".into())
                && !hgplain::is_plain(Some("color")),
        },
        Box::new(writer),
    )
    .map_err(|_| FallbackToPython("template not supported in Rust".to_owned()))
}

define_flags! {
    pub struct WalkOpts {
        /// include files matching the given patterns
        #[short('I')]
        #[argtype("PATTERN")]
        include: Vec<String>,

        /// exclude files matching the given patterns
        #[short('X')]
        #[argtype("PATTERN")]
        exclude: Vec<String>,
    }

    pub struct FormatterOpts {
        /// display with template (EXPERIMENTAL)
        #[short('T')]
        #[argtype("TEMPLATE")]
        template: String,
    }

    pub struct MergeToolOpts {
        /// specify merge tool
        #[short('t')]
        tool: String,
    }

    pub struct NoOpts {}
}
