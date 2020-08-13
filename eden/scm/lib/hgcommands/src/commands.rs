/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod debug;
mod root;
mod status;
mod version;

pub use anyhow::Result;
pub use clidispatch::io::IO;
pub use clidispatch::repo::Repo;
pub use cliparser::define_flags;

use clidispatch::command::{CommandTable, Register};

macro_rules! command_table {
    ( $( $module:ident $( :: $submodule:ident )* ),* ) => {{
        let mut table = CommandTable::new();
        $(
            // NOTE: Consider passing 'config' to name() or doc() if we want
            // some flexibility defining them using configs.
            {
                use self::$module $( :: $submodule )* as m;
                let command_name = m::name();
                let doc = m::doc();
                table.register(m::run, &command_name, &doc);
            }
        )*
        table
    }}
}

#[allow(dead_code)]
/// Return the main command table including all Rust commands.
pub fn table() -> CommandTable {
    command_table!(
        debug::args,
        debug::causerusterror,
        debug::dumpindexedlog,
        debug::dumptrace,
        debug::dynamicconfig,
        debug::http,
        debug::python,
        debug::store,
        root,
        status,
        version
    )
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
