/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

macro_rules! commands {
    ( $( mod $module:ident; )* ) => {
        $( mod $module; )*
        pub(crate) fn extend_command_table(table: &mut ::clidispatch::command::CommandTable) {
            // NOTE: Consider passing 'config' to name() or doc() if we want
            // some flexibility defining them using configs.
            $(
            {
                use self::$module as m;
                let command_aliases = m::aliases();
                let doc = m::doc();
                let synopsis = m::synopsis();
                ::clidispatch::command::Register::register(table, m::run, &command_aliases, &doc, synopsis.as_deref());
            }
            )*
        }
    }
}

mod debug;

commands! {
    mod clone;
    mod config;
    mod root;
    mod status;
    mod version;
    mod whereami;
}

use std::io::Write;

pub use anyhow::Result;
use clidispatch::command::CommandTable;
use clidispatch::errors::FallbackToPython;
use clidispatch::global_flags::HgGlobalOpts;
pub use clidispatch::io::IO;
pub use cliparser::define_flags;
pub use configparser::config::ConfigSet;
use formatter::formatter;
pub use repo::repo::Repo;

fn get_formatter(
    command_name: &'static str,
    template: &str,
    options: &HgGlobalOpts,
    writer: Box<dyn Write>,
) -> Result<Box<dyn formatter::ListFormatter>, FallbackToPython> {
    formatter::get_formatter(
        command_name,
        template,
        formatter::FormatOptions {
            debug: options.debug,
            verbose: options.verbose,
            quiet: options.quiet,
        },
        writer,
    )
    .map_err(|_| FallbackToPython("template not supported in Rust".to_owned()))
}

#[allow(dead_code)]
/// Return the main command table including all Rust commands.
pub fn table() -> CommandTable {
    let mut table = CommandTable::new();
    extend_command_table(&mut table);
    debug::extend_command_table(&mut table);

    table
}

define_flags! {
    pub struct WalkOpts {
        /// include names matching the given patterns
        #[short('I')]
        #[argtype("PATTERN")]
        include: Vec<String>,

        /// exclude names matching the given patterns
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

    pub struct NoOpts {}
}
