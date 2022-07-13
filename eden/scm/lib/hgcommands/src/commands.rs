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
                let command_name = m::name();
                let doc = m::doc();
                let synopsis = m::synopsis();
                ::clidispatch::command::Register::register(table, m::run, &command_name, &doc, synopsis.as_deref());
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

pub use anyhow::Result;
use clidispatch::command::CommandTable;
pub use clidispatch::io::IO;
pub use cliparser::define_flags;
pub use configparser::config::ConfigSet;
pub use repo::repo::Repo;

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
        include: Vec<String>,

        /// exclude names matching the given patterns
        #[short('X')]
        exclude: Vec<String>,
    }

    pub struct FormatterOpts {
        /// display with template (EXPERIMENTAL)
        #[short('T')]
        template: String,
    }

    pub struct NoOpts {}
}
