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

macro_rules! external_commands {
    [ $( $name:ident, )* ] => {
        pub(crate) fn extend_crate_command_table(table: &mut ::clidispatch::command::CommandTable) {
            $(
            {
                use ::$name as m;
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
    mod goto;
    mod root;
    mod status;
    mod version;
    mod whereami;
}

external_commands![
    // see update_commands.sh
    // [[[cog
    // import cog, glob, os
    // for path in sorted(glob.glob('commands/cmd*/TARGETS')):
    //     name = os.path.basename(os.path.dirname(path))
    //     cog.outl(f'{name},')
    // ]]]
    cmdclone,
    cmdconfig,
    cmdconfigfile,
    // [[[end]]]
];

#[cfg(feature = "fb")]
mod fb;

use clidispatch::command::CommandTable;

#[allow(dead_code)]
/// Return the main command table including all Rust commands.
pub fn table() -> CommandTable {
    let mut table = CommandTable::new();
    extend_command_table(&mut table);
    debug::extend_command_table(&mut table);

    extend_crate_command_table(&mut table);

    table
}
